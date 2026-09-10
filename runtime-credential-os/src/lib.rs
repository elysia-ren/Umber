//! 平台凭据后端（总案 §32 §32.1）。
//!
//! 回退顺序（契约要求）：
//!
//! ```text
//! 宿主提供的实现
//!     ↓ 缺失 / 失败
//! 操作系统钥匙串（Windows Credential Manager / macOS Keychain / libsecret）
//!     ↓ 缺失 / 失败
//! 加密文件存储 + 明确的用户告警
//! ```
//!
//! 设计要点：
//! - 本 crate 不引入 `unsafe`：OS 钥匙串经 `keyring` 抽象，加密文件用
//!   RustCrypto 的 ChaCha20-Poly1305。平台边界不因此泄漏到 Core。
//! - **加密文件层的密钥与数据同机存放**：它防的是"配置文件被顺手读走 /
//!   同步到云盘"，不防本机恶意软件。因此契约要求 UI 必须展示告警
//!   （`FallbackChain::active_tier()` 即告警依据），这不是可选的。

#![forbid(unsafe_code)]

pub mod chain;
pub mod encrypted_file;
pub mod os_keystore;

pub use chain::{CredentialTier, FallbackChain, TierFailure};
pub use encrypted_file::EncryptedFileStore;
pub use os_keystore::OsKeystore;
