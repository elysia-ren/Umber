//! CredentialStore 抽象（总案 §32）。
//!
//! - Core 不绑定操作系统；平台实现（Windows Credential Manager / Keychain /
//!   libsecret / 宿主提供）在 M9 落地，本 crate 提供契约与测试用内存实现。
//! - 回退顺序：宿主实现 → OS Keystore → 加密文件 + 告警。
//! - API Key 绝不进入普通配置 JSON / 日志 / Catalog / Canonical Request。
//!
//! 本 crate 同时提供 `redact` 工具：`raw_context` 与一切日志输出前
//! 必须脱敏（总案 §28）。

#![forbid(unsafe_code)]

pub mod redact;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

/// 凭据引用。Canonical Request 只携带引用，不携带秘密本身。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CredentialRef(pub String);

impl From<String> for CredentialRef {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for CredentialRef {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// 秘密值。Debug / Display 输出一律打码，防止意外进入日志。
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    NotFound,
    Backend(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::NotFound => f.write_str("credential not found"),
            CredentialError::Backend(m) => write!(f, "credential backend error: {m}"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Core 只知道四个操作（总案 §32）。
pub trait CredentialStore: Send + Sync {
    fn get(&self, reference: &CredentialRef) -> Result<Option<SecretString>, CredentialError>;
    fn set(&self, reference: &CredentialRef, secret: SecretString) -> Result<(), CredentialError>;
    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialError>;
    fn exists(&self, reference: &CredentialRef) -> Result<bool, CredentialError>;
}

/// 测试用内存实现。真实部署必须使用 OS Keystore / 宿主实现。
#[derive(Default)]
pub struct InMemoryCredentialStore {
    map: Mutex<BTreeMap<String, SecretString>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<SecretString>, CredentialError> {
        let map = self
            .map
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        Ok(map.get(&reference.0).cloned())
    }

    fn set(&self, reference: &CredentialRef, secret: SecretString) -> Result<(), CredentialError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        map.insert(reference.0.clone(), secret);
        Ok(())
    }

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        let mut map = self
            .map
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        Ok(map.remove(&reference.0).is_some())
    }

    fn exists(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        let map = self
            .map
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        Ok(map.contains_key(&reference.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_never_leaks_via_debug_or_display() {
        let s = SecretString::new("sk-super-secret");
        assert_eq!(format!("{s:?}"), "***");
        assert_eq!(format!("{s}"), "***");
        assert_eq!(s.expose(), "sk-super-secret");
    }

    #[test]
    fn memory_store_roundtrip() {
        let store = InMemoryCredentialStore::new();
        let r = CredentialRef("deepseek/api_key".into());
        assert!(!store.exists(&r).unwrap());
        store.set(&r, SecretString::new("sk-1")).unwrap();
        assert!(store.exists(&r).unwrap());
        assert_eq!(store.get(&r).unwrap().unwrap().expose(), "sk-1");
        assert!(store.delete(&r).unwrap());
        assert!(!store.delete(&r).unwrap());
        assert!(store.get(&r).unwrap().is_none());
    }
}
