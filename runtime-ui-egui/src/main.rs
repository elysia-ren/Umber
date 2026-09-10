//! settings-demo：点击"API 设置"弹出的默认设置窗口（真实后端）。
//!
//! 本 bin 演示完整的宿主接线：UISpec 渲染（runtime-ui-egui）与
//! 真实协议后端（runtime-protocol）。
//! 只做 Passive 动作（GET /models），不消耗 Token（总案 §17.1）。
use std::sync::Arc;

use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use runtime_model::deployment::ProtocolKind;
use runtime_protocol::{
    AnthropicAdapter, GeminiAdapter, HttpConfig, HttpTransport, OpenAiChatAdapter,
    OpenAiResponsesAdapter, RealHttpTransport,
};
use runtime_provider::ProviderAdapter;
use runtime_ui::{
    BackendError, ConnectionReport, SettingsBackend, SettingsDraft, UiModelEntry, UiModelInfo,
};
use runtime_ui_egui::{open_settings_window, Density, SettingsWindowParams, ThemeMode};

const KEY_REF: &str = "settings/api_key";

/// 真实后端：连接测试与发现 = Passive 的 GET /models（总案 §17.1：
/// 不发送生成请求、不调用 Tool、不消耗 Token——设置窗口的承诺）。
struct RealBackend;

fn endpoint_of(draft: &SettingsDraft) -> Result<runtime_model::deployment::Endpoint, BackendError> {
    let url = draft.values.get("endpoint").cloned().unwrap_or_default();
    if url.trim().is_empty() {
        return Err(BackendError::new("validation.required", "endpoint empty"));
    }
    Ok(runtime_model::deployment::Endpoint {
        id: "settings".into(),
        provider_id: draft
            .values
            .get("provider")
            .cloned()
            .unwrap_or_else(|| "custom".into()),
        url,
    })
}

fn protocol_of(draft: &SettingsDraft) -> ProtocolKind {
    match draft.values.get("protocol").map(|s| s.as_str()) {
        Some("openai_responses") => ProtocolKind::OpenAiResponses,
        Some("anthropic_messages") => ProtocolKind::AnthropicMessages,
        Some("gemini") => ProtocolKind::Gemini,
        _ => ProtocolKind::OpenAiChat,
    }
}

/// 从草稿组装本次会话的凭据：API Key 只存在内存里（总案 §32）。
/// 生产宿主应换成 `FallbackChain`（系统钥匙串优先）。
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
}

fn adapter_for(draft: &SettingsDraft) -> Arc<dyn ProviderAdapter> {
    let transport: Arc<dyn HttpTransport> = Arc::new(RealHttpTransport::new(HttpConfig::default()));
    match protocol_of(draft) {
        ProtocolKind::OpenAiResponses => Arc::new(OpenAiResponsesAdapter::new(transport)),
        ProtocolKind::AnthropicMessages => Arc::new(AnthropicAdapter::new(transport)),
        ProtocolKind::Gemini => Arc::new(GeminiAdapter::new(transport)),
        ProtocolKind::OpenAiChat | ProtocolKind::ProviderNative => {
            Arc::new(OpenAiChatAdapter::new(transport))
        }
    }
}

impl SettingsBackend for RealBackend {
    fn test_connection(&self, draft: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
        let endpoint = endpoint_of(draft)?;
        let credentials = credentials_of(draft);
        let adapter = adapter_for(draft);
        let started = std::time::Instant::now();
        // 连接测试 = GET /models（Passive；总案 §17.1）
        adapter
            .discover_models(&endpoint, &credentials, &CredentialRef::from(KEY_REF))
            .map_err(|e| BackendError::new("connection.failed", e.to_string()))?;
        Ok(ConnectionReport {
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    fn discover(&self, draft: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
        let endpoint = endpoint_of(draft)?;
        let credentials = credentials_of(draft);
        let adapter = adapter_for(draft);
        let models = adapter
            .discover_models(&endpoint, &credentials, &CredentialRef::from(KEY_REF))
            .map_err(|e| BackendError::new("connection.failed", e.to_string()))?;
        Ok(models
            .into_iter()
            .map(|m| UiModelEntry {
                model_id: m.model_id,
                display_name: m.display_name,
            })
            .collect())
    }

    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        // Passive 阶段能力全部未知——如实显示 Unknown（§15 §16）。
        // 确认能力是 Active Probe（显式开启 + 消耗 Token）的事。
        Some(UiModelInfo::all_unknown(model_id))
    }
}

fn main() {
    let params = SettingsWindowParams {
        title: "Universal Model Runtime — API 设置".into(),
        theme: ThemeMode::Dark,
        language: "zh-CN",
        scale: 1.0,
        density: Density::Cozy,
        page: runtime_ui::SettingsPage::provider_settings(),
        backend: Arc::new(RealBackend),
    };
    if let Err(e) = open_settings_window(params) {
        eprintln!("settings window failed: {e}");
        std::process::exit(1);
    }
}
