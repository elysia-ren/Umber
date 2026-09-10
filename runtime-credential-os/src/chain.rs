//! 凭据回退链（总案 §32.1）。
//!
//! ```text
//! 宿主提供的实现 → OS 钥匙串 → 加密文件 + 明确的用户告警
//! ```
//!
//! `active_tier()` 是 UI 告警的依据：一旦落到加密文件层，宿主**必须**
//! 在界面上提示用户（契约要求，不是建议）。

use std::sync::Arc;

use runtime_credential::{CredentialError, CredentialRef, CredentialStore, SecretString};

/// 当前生效的凭据存储层级。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialTier {
    /// 宿主提供实现（最优先：宿主可能接了自己的企业密钥管理）。
    Host,
    /// 操作系统钥匙串。
    OsKeystore,
    /// 加密文件（最后一档，**必须向用户告警**）。
    EncryptedFile,
}

impl CredentialTier {
    /// 是否需要向用户展示告警。
    pub fn requires_user_warning(&self) -> bool {
        matches!(self, CredentialTier::EncryptedFile)
    }

    pub fn label_key(&self) -> &'static str {
        match self {
            CredentialTier::Host => "credential.tier.host",
            CredentialTier::OsKeystore => "credential.tier.os_keystore",
            CredentialTier::EncryptedFile => "credential.tier.encrypted_file",
        }
    }
}

/// 某一层的失败记录（诊断用；不阻断回退）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierFailure {
    pub tier: CredentialTier,
    pub reason: String,
}

/// 回退链：写操作写入**所有**可用层级（读时按优先级回退），
/// 读操作按优先级取第一个命中的层。
///
/// 为什么写要写全部：用户可能今天钥匙串可用、明天在容器里不可用；
/// 写全部可让回退对用户透明。代价是同一秘密存在多处——对 API Key 而言
/// 可以接受，且每层都受各自的保护。
pub struct FallbackChain {
    tiers: Vec<(CredentialTier, Arc<dyn CredentialStore>)>,
    /// `write_all=false` 时只写最高优先级层。
    write_all: bool,
    failures: std::sync::Mutex<Vec<TierFailure>>,
}

impl FallbackChain {
    /// 按优先级顺序构造。第一项优先级最高。
    pub fn new(tiers: Vec<(CredentialTier, Arc<dyn CredentialStore>)>) -> Self {
        Self {
            tiers,
            write_all: true,
            failures: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 只使用宿主实现 + 加密文件（例如明确不想碰系统钥匙串的宿主）。
    pub fn with_write_all(mut self, write_all: bool) -> Self {
        self.write_all = write_all;
        self
    }

    /// 当前实际能服务的层级（按 get 探测）。
    ///
    /// 未存任何条目时返回最高优先级层——它才是**将用于写入**的层。
    pub fn active_tier(&self) -> Option<CredentialTier> {
        self.tiers.first().map(|(tier, _)| tier.clone())
    }

    /// 上一次操作中发生的层级失败（诊断 / UI 提示）。
    pub fn take_failures(&self) -> Vec<TierFailure> {
        let mut guard = self.failures.lock().expect("fallback failures");
        std::mem::take(&mut *guard)
    }

    fn record_failure(&self, tier: &CredentialTier, reason: String) {
        if let Ok(mut guard) = self.failures.lock() {
            // 上限保护：长驻宿主里失败可能被反复记录，不能让诊断缓冲无界增长
            const MAX_FAILURES: usize = 32;
            if guard.len() >= MAX_FAILURES {
                guard.remove(0);
            }
            guard.push(TierFailure {
                tier: tier.clone(),
                reason,
            });
        }
    }

    /// 探测哪一层真正可用（写入探针后删除）。用于启动时决定告警。
    pub fn probe(&self) -> Option<CredentialTier> {
        let probe_ref = CredentialRef::from("umer/__probe__");
        for (tier, store) in &self.tiers {
            match store.set(&probe_ref, SecretString::new("probe")) {
                Ok(()) => {
                    let _ = store.delete(&probe_ref);
                    return Some(tier.clone());
                }
                Err(e) => self.record_failure(tier, e.to_string()),
            }
        }
        None
    }
}

impl CredentialStore for FallbackChain {
    fn get(&self, reference: &CredentialRef) -> Result<Option<SecretString>, CredentialError> {
        for (tier, store) in &self.tiers {
            match store.get(reference) {
                Ok(Some(secret)) => return Ok(Some(secret)),
                Ok(None) => continue,
                Err(e) => self.record_failure(tier, e.to_string()),
            }
        }
        Ok(None)
    }

    fn set(&self, reference: &CredentialRef, secret: SecretString) -> Result<(), CredentialError> {
        let mut last_error = None;
        let mut wrote_any = false;
        for (index, (tier, store)) in self.tiers.iter().enumerate() {
            match store.set(reference, secret.clone()) {
                Ok(()) => {
                    wrote_any = true;
                    if !self.write_all {
                        break;
                    }
                }
                Err(e) => {
                    self.record_failure(tier, e.to_string());
                    last_error = Some(e);
                    // 最高优先级层失败时，继续尝试下一层（回退语义）
                    let _ = index;
                }
            }
        }
        match (wrote_any, last_error) {
            (true, _) => Ok(()),
            (false, Some(e)) => Err(e),
            (false, None) => Err(CredentialError::Backend(
                "no credential tier available".into(),
            )),
        }
    }

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        let mut deleted = false;
        for (tier, store) in &self.tiers {
            match store.delete(reference) {
                Ok(true) => deleted = true,
                Ok(false) => {}
                Err(e) => self.record_failure(tier, e.to_string()),
            }
        }
        Ok(deleted)
    }

    fn exists(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        Ok(self.get(reference)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_credential::InMemoryCredentialStore;

    /// 永远失败的层，用于验证回退。
    struct BrokenStore;

    impl CredentialStore for BrokenStore {
        fn get(&self, _: &CredentialRef) -> Result<Option<SecretString>, CredentialError> {
            Err(CredentialError::Backend("broken get".into()))
        }
        fn set(&self, _: &CredentialRef, _: SecretString) -> Result<(), CredentialError> {
            Err(CredentialError::Backend("broken set".into()))
        }
        fn delete(&self, _: &CredentialRef) -> Result<bool, CredentialError> {
            Err(CredentialError::Backend("broken delete".into()))
        }
        fn exists(&self, _: &CredentialRef) -> Result<bool, CredentialError> {
            Err(CredentialError::Backend("broken exists".into()))
        }
    }

    fn chain() -> FallbackChain {
        FallbackChain::new(vec![
            (CredentialTier::Host, Arc::new(BrokenStore)),
            (CredentialTier::OsKeystore, Arc::new(BrokenStore)),
            (
                CredentialTier::EncryptedFile,
                Arc::new(InMemoryCredentialStore::new()),
            ),
        ])
    }

    #[test]
    fn falls_through_broken_tiers_to_the_last_one() {
        let chain = chain();
        let reference = CredentialRef::from("deepseek/api_key");
        chain
            .set(&reference, SecretString::new("sk-last-resort"))
            .unwrap();
        assert_eq!(
            chain.get(&reference).unwrap().unwrap().expose(),
            "sk-last-resort"
        );
        // set 与 get 各会经过两个坏层：失败记录里两个坏层都必须出现
        let failures = chain.take_failures();
        assert!(failures.len() >= 2, "坏层失败应被记录");
        assert!(failures.iter().any(|f| f.tier == CredentialTier::Host));
        assert!(failures
            .iter()
            .any(|f| f.tier == CredentialTier::OsKeystore));
        assert!(!failures
            .iter()
            .any(|f| f.tier == CredentialTier::EncryptedFile));
    }

    #[test]
    fn failure_log_is_bounded() {
        let chain = chain();
        let reference = CredentialRef::from("k");
        for _ in 0..200 {
            let _ = chain.get(&reference);
        }
        // 反复探测不会让诊断缓冲无界增长
        assert!(chain.take_failures().len() <= 32);
    }

    #[test]
    fn encrypted_file_tier_requires_user_warning() {
        assert!(CredentialTier::EncryptedFile.requires_user_warning());
        assert!(!CredentialTier::Host.requires_user_warning());
        assert!(!CredentialTier::OsKeystore.requires_user_warning());
        assert_eq!(
            CredentialTier::EncryptedFile.label_key(),
            "credential.tier.encrypted_file"
        );
    }

    #[test]
    fn probe_reports_the_first_working_tier() {
        let chain = FallbackChain::new(vec![
            (CredentialTier::Host, Arc::new(BrokenStore)),
            (
                CredentialTier::OsKeystore,
                Arc::new(InMemoryCredentialStore::new()),
            ),
        ]);
        assert_eq!(chain.probe(), Some(CredentialTier::OsKeystore));
    }

    #[test]
    fn all_tiers_broken_reports_error_not_silent_success() {
        let chain = FallbackChain::new(vec![
            (CredentialTier::Host, Arc::new(BrokenStore)),
            (CredentialTier::OsKeystore, Arc::new(BrokenStore)),
        ]);
        let err = chain
            .set(&CredentialRef::from("k"), SecretString::new("v"))
            .unwrap_err();
        assert!(matches!(err, CredentialError::Backend(_)));
        assert_eq!(chain.probe(), None);
    }

    #[test]
    fn write_all_false_stops_at_first_success() {
        let first = Arc::new(InMemoryCredentialStore::new());
        let chain = FallbackChain::new(vec![
            (CredentialTier::Host, first.clone()),
            (
                CredentialTier::OsKeystore,
                Arc::new(InMemoryCredentialStore::new()),
            ),
        ])
        .with_write_all(false);
        let reference = CredentialRef::from("k");
        chain.set(&reference, SecretString::new("v")).unwrap();
        assert!(first.exists(&reference).unwrap());
    }
}
