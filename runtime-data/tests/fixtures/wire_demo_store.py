"""demo 后端接入 Runtime Local DB + 凭据回退链，实现真实的 save/load。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\main.rs"
m = io.open(P, encoding="utf-8").read()

# 1) 依赖与导入
m = m.replace(
    """use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};""",
    """use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use runtime_credential_os::{CredentialTier, EncryptedFileStore, FallbackChain, OsKeystore};
use runtime_data::{LocalDb, StoredDeployment, UserOverride};""",
)
m = m.replace(
    """use runtime_ui::{
    BackendError, ConnectionReport, SettingsBackend, SettingsDraft, UiModelEntry, UiModelInfo,
};""",
    """use runtime_ui::{
    BackendError, ConnectionReport, SaveReport, SavedSettings, SettingsBackend, SettingsDraft,
    UiModelEntry, UiModelInfo,
};""",
)

# 2) RealBackend 增加持久化层
m = m.replace(
    """    /// provider → 该 provider 暴露的模型 ID（规格 X.18 的 Deployment 表）。
    /// 推荐模型据此得出——**查表，不是硬编码模型名**。
    provider_models: HashMap<String, Vec<String>>,
}""",
    """    /// provider → 该 provider 暴露的模型 ID（规格 X.18 的 Deployment 表）。
    /// 推荐模型据此得出——**查表，不是硬编码模型名**。
    provider_models: HashMap<String, Vec<String>>,
    /// 运行时本地库（配置落盘）与凭据回退链（密钥进系统钥匙串）。
    store: std::sync::Mutex<Option<StoreBundle>>,
}

/// 本地持久化：Local DB + 凭据回退链。
///
/// 打开失败（权限、磁盘）不致命：内存态仍可用，只是保存会返回错误。
struct StoreBundle {
    db: LocalDb,
    credentials: FallbackChain,
    credential_ref: CredentialRef,
}

/// 数据目录：Windows 用 `%APPDATA%`，其他平台用 `$XDG_DATA_HOME` / `~/.local/share`。
fn data_dir() -> std::path::PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return std::path::PathBuf::from(appdata).join("UniversalModelRuntime");
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("universal-model-runtime");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".local/share/universal-model-runtime")
}

impl StoreBundle {
    fn open() -> Option<Self> {
        let dir = data_dir();
        let db = LocalDb::open(&dir).ok()?;
        let credentials_dir = dir.join("credentials");
        let mut tiers: Vec<(CredentialTier, Arc<dyn CredentialStore>)> = Vec::new();
        // 回退顺序（§32.1）：宿主实现 → 系统钥匙串 → 加密文件
        let keystore = OsKeystore::new();
        if keystore.is_available() {
            tiers.push((CredentialTier::OsKeystore, Arc::new(keystore)));
        }
        if let Ok(encrypted) = EncryptedFileStore::open(&credentials_dir) {
            tiers.push((CredentialTier::EncryptedFile, Arc::new(encrypted)));
        }
        if tiers.is_empty() {
            return None;
        }
        Some(Self {
            db,
            credentials: FallbackChain::new(tiers),
            credential_ref: CredentialRef::from("settings/api_key"),
        })
    }
}""",
)

# 3) new() 里打开 store
m = m.replace(
    """            provider_models: load_provider_models_from_env(),
        }
    }""",
    """            provider_models: load_provider_models_from_env(),
            store: std::sync::Mutex::new(StoreBundle::open()),
        }
    }""",
)

# 4) 实现 load_settings / save_settings
m = m.replace(
    """    /// URL 预览直接调用 Adapter 的 URL 函数——与真实请求同源，不会漂移。""",
    """    /// 读上次保存的配置。密钥只回"有没有"，不回值（§32）。
    fn load_settings(&self) -> Option<SavedSettings> {
        let guard = self.store.lock().ok()?;
        let bundle = guard.as_ref()?;
        let deployments = bundle.db.deployments().ok()?;
        let deployment = deployments
            .iter()
            .max_by_key(|d| d.created_at_unix)?;
        let context_window = bundle
            .db
            .override_for(&deployment.model_id, "context_window")
            .ok()
            .flatten()
            .and_then(|o| o.value.parse::<u64>().ok());
        let has_api_key = bundle
            .credentials
            .exists(&bundle.credential_ref)
            .unwrap_or(false);
        Some(SavedSettings {
            provider: deployment.provider.clone(),
            protocol: deployment.protocol.clone(),
            endpoint: deployment.endpoint.clone(),
            model_id: Some(deployment.model_id.clone()),
            context_window,
            has_api_key,
        })
    }

    /// 保存配置：配置项进 Local DB，密钥进凭据存储（**绝不写进配置**）。
    fn save_settings(
        &self,
        draft: &SettingsDraft,
        api_key: &str,
    ) -> Result<SaveReport, BackendError> {
        let guard = self
            .store
            .lock()
            .map_err(|_| BackendError::new("settings.save_failed", "store lock poisoned"))?;
        let bundle = guard.as_ref().ok_or_else(|| {
            BackendError::new(
                "settings.save_failed",
                format!("cannot open data dir: {}", data_dir().display()),
            )
        })?;

        let provider = draft.values.get("provider").cloned().unwrap_or_default();
        let protocol = draft.values.get("protocol").cloned().unwrap_or_default();
        let endpoint = draft.values.get("endpoint").cloned().unwrap_or_default();
        let model_id = draft.values.get("model").cloned().unwrap_or_default();
        if endpoint.trim().is_empty() || model_id.trim().is_empty() {
            return Err(BackendError::new(
                "validation.endpoint.required",
                "endpoint and model are required before saving",
            ));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        bundle
            .db
            .upsert_deployment(&StoredDeployment {
                id: format!("{provider}/{model_id}").into(),
                provider: provider.clone(),
                endpoint: endpoint.clone(),
                protocol: protocol.clone(),
                model_id: model_id.clone(),
                // 存引用，不存密钥本身
                credential_ref: Some(bundle.credential_ref.0.clone()),
                created_at_unix: now,
            })
            .map_err(|e| BackendError::new("settings.save_failed", e.to_string()))?;

        // 上下文覆盖走 User Override（用户覆盖不删原始知识，§X.22）
        if let Some(context) = draft.values.get("context_window") {
            if !context.trim().is_empty() && context.trim() != "0" {
                bundle
                    .db
                    .set_override(&UserOverride {
                        model_id: model_id.clone(),
                        field: "context_window".into(),
                        value: context.trim().to_string(),
                        created_at_unix: now,
                    })
                    .map_err(|e| BackendError::new("settings.save_failed", e.to_string()))?;
            }
        }

        // 密钥：只有用户这次填了才写（空 = 不改动已保存的密钥）
        let mut saved_credential = false;
        let mut warning = None;
        if !api_key.trim().is_empty() {
            bundle
                .credentials
                .set(&bundle.credential_ref, SecretString::new(api_key.trim()))
                .map_err(|e| BackendError::new("settings.save_failed", e.to_string()))?;
            saved_credential = true;
            // 落到加密文件层必须提示用户（§32.1：这不是可选的美化）
            if let Some(tier) = bundle.credentials.probe() {
                if tier.requires_user_warning() {
                    warning = Some(tier.label_key().to_string());
                }
            }
        }

        Ok(SaveReport {
            saved_config: true,
            saved_credential,
            credential_warning_key: warning,
        })
    }

    /// URL 预览直接调用 Adapter 的 URL 函数——与真实请求同源，不会漂移。""",
)

# 5) credentials_of 也要能用已保存的密钥（否则刷新/测试连接在重启后失败）
m = m.replace(
    """    /// 本次会话的凭据（API Key 只存在内存，§32）。
    fn credentials_of(draft: &SettingsDraft) -> InMemoryCredentialStore {
        let store = InMemoryCredentialStore::new();
        if let Some(key) = draft
            .values
            .get("api_key")
            .filter(|k| !k.trim().is_empty())
            .cloned()
        {
            let _ = store.set(&CredentialRef::from(KEY_REF), SecretString::new(key));
        }
        store
    }""",
    """    /// 本次会话的凭据：优先用用户刚输入的，否则回落到已保存的（§32）。
    ///
    /// 只读回内存供本次调用使用，密钥不进入任何配置文件。
    fn credentials_for(&self, draft: &SettingsDraft) -> InMemoryCredentialStore {
        let store = InMemoryCredentialStore::new();
        let provided = draft
            .values
            .get("api_key")
            .filter(|k| !k.trim().is_empty())
            .cloned();
        let key = provided.or_else(|| {
            let guard = self.store.lock().ok()?;
            let bundle = guard.as_ref()?;
            bundle
                .credentials
                .get(&bundle.credential_ref)
                .ok()
                .flatten()
                .map(|secret| secret.expose().to_string())
        });
        if let Some(key) = key {
            let _ = store.set(&CredentialRef::from(KEY_REF), SecretString::new(key));
        }
        store
    }""",
)
m = m.replace("Self::credentials_of(draft)", "self.credentials_for(draft)")

io.open(P, "w", encoding="utf-8", newline="\n").write(m)
print("main.rs patched")
