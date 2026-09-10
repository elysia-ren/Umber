//! 加密文件凭据存储（总案 §32.1 的最后一档回退）。
//!
//! **安全边界必须说清楚**：密钥文件与数据文件在同一台机器上，
//! 因此这层防护的目标是
//!
//! ```text
//! 防：配置文件被顺手读走、被同步到云盘、被截图/日志带出
//! 不防：本机上的恶意软件（它能同时读到密钥文件）
//! ```
//!
//! 所以契约把"明确的用户告警"写成硬要求：启用这一层时 UI 必须提示
//! 用户（`FallbackChain::active_tier()` 提供依据）。
//!
//! 加密：ChaCha20-Poly1305，每条记录独立随机 nonce；
//! 密钥文件与数据文件分离（`key.bin` / `credentials.json`），权限 0600。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;

use runtime_credential::{CredentialError, CredentialRef, CredentialStore, SecretString};

const KEY_FILE: &str = "key.bin";
const DATA_FILE: &str = "credentials.json";

/// 磁盘目录中的加密凭据存储。
pub struct EncryptedFileStore {
    dir: PathBuf,
    key: [u8; 32],
    cache: std::sync::Mutex<BTreeMap<String, String>>,
}

impl EncryptedFileStore {
    /// 打开（或首次创建）指定目录下的加密存储。
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, CredentialError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(backend)?;
        let key_path = dir.join(KEY_FILE);
        let key = if key_path.exists() {
            let bytes = fs::read(&key_path).map_err(backend)?;
            if bytes.len() != 32 {
                return Err(CredentialError::Backend(format!(
                    "corrupted key file at {}",
                    key_path.display()
                )));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            key
        } else {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            fs::write(&key_path, key).map_err(backend)?;
            restrict_permissions(&key_path)?;
            key
        };
        let store = Self {
            dir,
            key,
            cache: std::sync::Mutex::new(BTreeMap::new()),
        };
        store.reload()?;
        Ok(store)
    }

    fn data_path(&self) -> PathBuf {
        self.dir.join(DATA_FILE)
    }

    /// 从磁盘解密全部条目到内存缓存。
    fn reload(&self) -> Result<(), CredentialError> {
        let path = self.data_path();
        if !path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&path).map_err(backend)?;
        let records: BTreeMap<String, StoredRecord> = serde_json::from_str(&raw)
            .map_err(|e| CredentialError::Backend(format!("corrupted credential file: {e}")))?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        cache.clear();
        for (key, record) in records {
            let nonce = decode_hex(&record.nonce)?;
            if nonce.len() != 12 {
                return Err(CredentialError::Backend("bad nonce length".into()));
            }
            let ciphertext = decode_hex(&record.ciphertext)?;
            let plaintext = cipher
                .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
                .map_err(|_| CredentialError::Backend("decryption failed (wrong key?)".into()))?;
            let value = String::from_utf8(plaintext)
                .map_err(|_| CredentialError::Backend("stored value is not UTF-8".into()))?;
            cache.insert(key, value);
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), CredentialError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let mut records: BTreeMap<String, StoredRecord> = BTreeMap::new();
        for (key, value) in cache.iter() {
            let mut nonce_bytes = [0u8; 12];
            OsRng.fill_bytes(&mut nonce_bytes);
            let ciphertext = cipher
                .encrypt(Nonce::from_slice(&nonce_bytes), value.as_bytes())
                .map_err(|_| CredentialError::Backend("encryption failed".into()))?;
            records.insert(
                key.clone(),
                StoredRecord {
                    nonce: encode_hex(&nonce_bytes),
                    ciphertext: encode_hex(&ciphertext),
                },
            );
        }
        let json = serde_json::to_string_pretty(&records)
            .map_err(|e| CredentialError::Backend(format!("serialize: {e}")))?;
        let path = self.data_path();
        fs::write(&path, json).map_err(backend)?;
        restrict_permissions(&path)?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredRecord {
    nonce: String,
    ciphertext: String,
}

fn backend(e: std::io::Error) -> CredentialError {
    CredentialError::Backend(e.to_string())
}

/// 尽力收紧权限（Unix 0600；Windows 继承用户 ACL）。
fn restrict_permissions(path: &Path) -> Result<(), CredentialError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).map_err(backend)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms).map_err(backend)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(text: &str) -> Result<Vec<u8>, CredentialError> {
    if text.len() % 2 != 0 {
        return Err(CredentialError::Backend("odd hex length".into()));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| CredentialError::Backend("invalid hex".into()))
        })
        .collect()
}

impl CredentialStore for EncryptedFileStore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<SecretString>, CredentialError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        Ok(cache.get(&reference.0).map(SecretString::new))
    }

    fn set(&self, reference: &CredentialRef, secret: SecretString) -> Result<(), CredentialError> {
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| CredentialError::Backend("poisoned".into()))?;
            cache.insert(reference.0.clone(), secret.expose().to_string());
        }
        self.flush()
    }

    fn delete(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        let removed = {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| CredentialError::Backend("poisoned".into()))?;
            cache.remove(&reference.0).is_some()
        };
        if removed {
            self.flush()?;
        }
        Ok(removed)
    }

    fn exists(&self, reference: &CredentialRef) -> Result<bool, CredentialError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| CredentialError::Backend("poisoned".into()))?;
        Ok(cache.contains_key(&reference.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "umer-cred-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        dir
    }

    #[test]
    fn round_trip_survives_reopen() {
        let dir = temp_dir("roundtrip");
        let reference = CredentialRef::from("deepseek/api_key");
        {
            let store = EncryptedFileStore::open(&dir).unwrap();
            assert!(!store.exists(&reference).unwrap());
            store
                .set(&reference, SecretString::new("sk-secret-1"))
                .unwrap();
        }
        // 重新打开：密钥复用，数据可解密
        let store = EncryptedFileStore::open(&dir).unwrap();
        assert_eq!(
            store.get(&reference).unwrap().unwrap().expose(),
            "sk-secret-1"
        );
        assert!(store.delete(&reference).unwrap());
        assert!(!store.exists(&reference).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn plaintext_never_touches_disk() {
        let dir = temp_dir("plaintext");
        let store = EncryptedFileStore::open(&dir).unwrap();
        store
            .set(
                &CredentialRef::from("p/key"),
                SecretString::new("PLAINTEXT-MARKER-12345"),
            )
            .unwrap();
        let data = fs::read_to_string(dir.join(DATA_FILE)).unwrap();
        assert!(
            !data.contains("PLAINTEXT-MARKER"),
            "明文绝不能出现在磁盘文件里"
        );
        assert!(data.contains("ciphertext"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_key_cannot_decrypt() {
        let dir = temp_dir("wrongkey");
        {
            let store = EncryptedFileStore::open(&dir).unwrap();
            store
                .set(&CredentialRef::from("k"), SecretString::new("v"))
                .unwrap();
        }
        // 换一把密钥 → 解密必须失败，而不是返回垃圾数据
        fs::write(dir.join(KEY_FILE), [7u8; 32]).unwrap();
        let result = EncryptedFileStore::open(&dir);
        assert!(result.is_err(), "错误密钥必须报错");
        let _ = fs::remove_dir_all(&dir);
    }
}
