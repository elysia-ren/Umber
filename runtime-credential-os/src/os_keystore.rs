//! OS 钥匙串后端（总案 §32）。
//!
//! - Windows → Windows Credential Manager
//! - macOS   → Keychain
//! - Linux   → libsecret（secret-service）
//!
//! 经 `keyring` 抽象，本 crate 保持零 `unsafe`。

use runtime_credential::{CredentialError, CredentialRef, CredentialStore, SecretString};

/// 钥匙串中的服务名（Windows 的 target、macOS 的 service）。
pub const SERVICE: &str = "UniversalEmbeddedModelRuntime";

/// 把 `CredentialRef` 映射为钥匙串账号名。
///
/// 保留原始字符串（不做哈希/截断），因为用户需要在系统钥匙串里
/// 认出"哪一条是我的 DeepSeek Key"——可识别性优先于简洁。
fn account_of(reference: &CredentialRef) -> String {
    reference.0.clone()
}

pub struct OsKeystore {
    service: String,
}

impl OsKeystore {
    pub fn new() -> Self {
        Self {
            service: SERVICE.to_string(),
        }
    }

    /// 自定义服务名（多宿主共用一台机器时按宿主隔离条目）。
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// 系统钥匙串是否可用。用于回退链决策与 UI 提示。
    pub fn is_available(&self) -> bool {
        entry(&self.service, "umer-probe").is_ok()
    }
}

impl Default for OsKeystore {
    fn default() -> Self {
        Self::new()
    }
}

fn entry(service: &str, account: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(service, account).map_err(|e| CredentialError::Backend(e.to_string()))
}

fn is_not_found(e: &keyring::Error) -> bool {
    matches!(e, keyring::Error::NoEntry)
}

impl CredentialStore for OsKeystore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<SecretString>, CredentialError> {
        match entry(&self.service, &account_of(reference))?.get_password() {
            Ok(password) => Ok(Some(SecretString::new(password))),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }

    fn set(&self, reference: &CredentialRef, secret: SecretString) -> Result<(), CredentialError> {
        entry(&self.service, &account_of(reference))?
            .set_password(secret.expose())
            .map_err(|e| CredentialError::Backend(e.to_string()))
    }

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        match entry(&self.service, &account_of(reference))?.delete_credential() {
            Ok(()) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }

    fn exists(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        Ok(self.get(reference)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_name_is_human_identifiable() {
        // 用户要能在系统钥匙串里认出条目，因此不做哈希
        let r = CredentialRef::from("deepseek/api_key");
        assert_eq!(account_of(&r), "deepseek/api_key");
    }

    #[test]
    fn store_constructs_with_custom_service() {
        let store = OsKeystore::with_service("MyHost");
        assert_eq!(store.service, "MyHost");
        let _ = OsKeystore::new();
    }

    /// 真实写入系统钥匙串。默认忽略（会污染开发机的凭据管理器）：
    /// `cargo test -p runtime-credential-os -- --ignored`
    #[test]
    #[ignore = "writes to the real OS keystore"]
    fn os_keystore_round_trip() {
        let store = OsKeystore::with_service("UniversalEmbeddedModelRuntimeTest");
        let reference = CredentialRef::from("test/smoke-key");
        store.delete(&reference).ok();
        assert!(!store.exists(&reference).unwrap());
        store
            .set(&reference, SecretString::new("value-123"))
            .unwrap();
        assert!(store.exists(&reference).unwrap());
        assert_eq!(
            store.get(&reference).unwrap().unwrap().expose(),
            "value-123"
        );
        assert!(store.delete(&reference).unwrap());
        assert!(!store.exists(&reference).unwrap());
    }
}
