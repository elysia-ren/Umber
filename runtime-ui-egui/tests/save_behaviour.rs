//! 保存/恢复的行为测试。
//!
//! 之前"没有保存"是因为保存按钮什么都没做，且**没有任何测试约束它**。
//! 这里用真实的 Runtime Local DB + 内存凭据存储验证端到端持久化：
//! 保存 → 换一个新的 app 实例（模拟重启）→ 配置与密钥状态被恢复。

use std::sync::Arc;

use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use runtime_data::{LocalDb, StoredDeployment, UserOverride};
use runtime_ui::{
    BackendError, ConnectionReport, SaveReport, SavedSettings, SettingsBackend, SettingsDraft,
    UiModelEntry, UiModelInfo,
};
use runtime_ui_egui::{SaveState, SettingsApp};

/// 基于真实 LocalDb 的持久化后端（凭据用内存实现，避免测试写入真实钥匙串）。
struct PersistingBackend {
    db: LocalDb,
    credentials: InMemoryCredentialStore,
    credential_ref: CredentialRef,
    /// 可选的保存失败注入
    fail_save: bool,
}

impl PersistingBackend {
    fn new(dir: &std::path::Path) -> Arc<Self> {
        Arc::new(Self {
            db: LocalDb::open(dir).expect("local db opens"),
            credentials: InMemoryCredentialStore::new(),
            credential_ref: CredentialRef::from("settings/api_key"),
            fail_save: false,
        })
    }

    fn failing(dir: &std::path::Path) -> Arc<Self> {
        Arc::new(Self {
            db: LocalDb::open(dir).expect("local db opens"),
            credentials: InMemoryCredentialStore::new(),
            credential_ref: CredentialRef::from("settings/api_key"),
            fail_save: true,
        })
    }
}

impl SettingsBackend for PersistingBackend {
    fn test_connection(
        &self,
        _: &SettingsDraft,
        _: Option<&str>,
    ) -> Result<ConnectionReport, BackendError> {
        Ok(ConnectionReport { latency_ms: 1 })
    }

    fn discover(
        &self,
        _: &SettingsDraft,
        _: Option<&str>,
    ) -> Result<Vec<UiModelEntry>, BackendError> {
        Ok(vec![])
    }

    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        Some(UiModelInfo::unknown(model_id))
    }

    fn load_settings(&self) -> Option<SavedSettings> {
        let deployments = self.db.deployments().ok()?;
        let deployment = deployments.iter().max_by_key(|d| d.created_at_unix)?;
        let context_window = self
            .db
            .override_for(&deployment.model_id, "context_window")
            .ok()
            .flatten()
            .and_then(|o| o.value.parse::<u64>().ok());
        let has_api_key = self
            .credentials
            .exists(&self.credential_ref)
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

    fn save_settings(
        &self,
        draft: &SettingsDraft,
        api_key: &str,
    ) -> Result<SaveReport, BackendError> {
        if self.fail_save {
            return Err(BackendError::new(
                "settings.save_failed",
                "disk full (injected)",
            ));
        }
        let endpoint = draft.values.get("endpoint").cloned().unwrap_or_default();
        let model_id = draft.values.get("model").cloned().unwrap_or_default();
        if endpoint.trim().is_empty() || model_id.trim().is_empty() {
            return Err(BackendError::new(
                "validation.endpoint.required",
                "endpoint and model required",
            ));
        }
        self.db
            .upsert_deployment(&StoredDeployment {
                id: model_id.clone().into(),
                provider: draft.values.get("provider").cloned().unwrap_or_default(),
                endpoint,
                protocol: draft.values.get("protocol").cloned().unwrap_or_default(),
                model_id: model_id.clone(),
                credential_ref: Some(self.credential_ref.0.clone()),
                created_at_unix: 10,
            })
            .map_err(|e| BackendError::new("settings.save_failed", e.to_string()))?;
        if let Some(context) = draft.values.get("context_window") {
            if !context.trim().is_empty() {
                self.db
                    .set_override(&UserOverride {
                        model_id: model_id.clone(),
                        field: "context_window".into(),
                        value: context.trim().to_string(),
                        created_at_unix: 10,
                    })
                    .map_err(|e| BackendError::new("settings.save_failed", e.to_string()))?;
            }
        }
        let saved_credential = !api_key.trim().is_empty();
        if saved_credential {
            self.credentials
                .set(&self.credential_ref, SecretString::new(api_key.trim()))
                .map_err(|e| BackendError::new("settings.save_failed", e.to_string()))?;
        }
        Ok(SaveReport {
            saved_config: true,
            saved_credential,
            credential_warning_key: None,
        })
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "umer-save-test-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    dir
}

fn app_with(backend: Arc<PersistingBackend>) -> SettingsApp {
    SettingsApp::new(
        runtime_ui::SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("zh-CN").unwrap(),
        backend,
    )
}

fn wait_idle(app: &mut SettingsApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        app.poll_pending();
        if !app.is_busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("后台动作超时");
}

#[test]
fn save_then_reopen_restores_the_configuration() {
    let dir = temp_dir("roundtrip");
    let backend = PersistingBackend::new(&dir);

    // 第一次打开：改配置、填密钥、保存
    {
        let mut app = app_with(backend.clone());
        app.load_saved(); // 首启：无保存内容
        app.state_mut().select_provider("zhipu");
        app.state_mut()
            .set_endpoint("https://my-gateway.example/v1");
        // 顺序与真实用户一致：先选模型，再设上下文覆盖。
        // （select_model 会清空覆盖——换模型后上下文当然不同，这是有意的行为）
        app.state_mut().select_model("glm-4.6");
        app.state_mut().set_context_override_input("200000");
        app.state_mut().set_api_key("sk-TEST-KEY");
        assert!(app.state().is_ready(), "配置齐全才能保存");

        app.trigger_save();
        wait_idle(&mut app);
        match app.save_state() {
            SaveState::Saved { credential } => assert!(*credential, "填了密钥就该一并保存"),
            other => panic!("保存应成功，实际 {other:?}"),
        }
    }

    // 第二次打开：模拟重启，配置与"已有密钥"都被恢复
    {
        let mut app = app_with(backend.clone());
        app.load_saved();
        assert_eq!(app.state().selected_provider_id(), "zhipu");
        assert_eq!(app.state().endpoint(), "https://my-gateway.example/v1");
        assert_eq!(app.state().selected_model(), Some("glm-4.6"));
        assert_eq!(app.state().context_override_input(), "200000");
        assert!(app.state().has_saved_key(), "应知道已有密钥");
        // 界面状态里不该留着密钥明文
        assert_eq!(app.state().api_key(), "");
        assert!(!app.state().needs_key_input(), "已有密钥就不该再要求填写");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_failure_is_visible_and_does_not_claim_success() {
    let dir = temp_dir("fail");
    let backend = PersistingBackend::failing(&dir);
    let mut app = app_with(backend);
    app.state_mut().select_provider("deepseek");
    app.state_mut().select_model("deepseek-chat");

    app.trigger_save();
    wait_idle(&mut app);
    match app.save_state() {
        SaveState::Failed(reason) => {
            assert!(reason.contains("disk full"), "应带上后端给的原因: {reason}");
        }
        other => panic!("保存失败必须是 Failed 而不是自称成功，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saving_without_a_key_does_not_wipe_the_saved_one() {
    let dir = temp_dir("keepkey");
    let backend = PersistingBackend::new(&dir);
    {
        let mut app = app_with(backend.clone());
        app.state_mut().select_provider("deepseek");
        app.state_mut().select_model("deepseek-chat");
        app.state_mut().set_api_key("sk-KEEP-ME");
        app.trigger_save();
        wait_idle(&mut app);
    }
    // 再次保存但不输入密钥（用户只是改了别的东西）
    {
        let mut app = app_with(backend.clone());
        app.load_saved();
        assert!(app.state().has_saved_key());
        app.state_mut().set_endpoint("https://relay.example/v1");
        app.trigger_save();
        wait_idle(&mut app);
        assert!(matches!(app.save_state(), SaveState::Saved { .. }));
    }
    // 密钥仍然在
    let mut app = app_with(backend.clone());
    app.load_saved();
    assert!(
        app.state().has_saved_key(),
        "未重新输入密钥时不该把已保存的密钥清掉"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn contexts_override_is_persisted_as_user_override() {
    let dir = temp_dir("override");
    let backend = PersistingBackend::new(&dir);
    let mut app = app_with(backend.clone());
    app.state_mut().select_provider("deepseek");
    app.state_mut().select_model("deepseek-chat");
    app.state_mut().set_context_override_input("131072");
    app.trigger_save();
    wait_idle(&mut app);

    // 覆盖值进的是 user_overrides 表（原始知识照旧保留，§X.22）
    let stored = backend
        .db
        .override_for("deepseek-chat", "context_window")
        .unwrap()
        .expect("覆盖应被记录");
    assert_eq!(stored.value, "131072");
    assert_eq!(stored.model_id, "deepseek-chat");
    let _ = std::fs::remove_dir_all(&dir);
}
