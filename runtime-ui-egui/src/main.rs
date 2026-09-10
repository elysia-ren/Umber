//! settings-demo：设置窗口的完整宿主接线（真实后端）。
//!
//! 数据来源：
//! - **模型知识**：由 `model-data build` 产出的 Canonical Catalog
//!   （实测：6158 个模型，含能力/上下文/价格/档位/证据）。
//!   用环境变量 `UMER_DEMO_CATALOG` 指定路径；未提供时退回"全未知"，
//!   界面会如实显示"该模型无目录数据"而不是编造。
//! - **协议与网络**：`runtime-protocol` 的真实适配器。
//! - URL 预览直接调用适配器的 URL 构造函数——**预览与实际请求不可能漂移**。
//!
//! 只做 Passive 动作（GET /models），不消耗 Token（总案 §17.1）。

use std::collections::HashMap;
use std::sync::Arc;

use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use runtime_model::capability::{CapabilityKind, CapabilityStatus};
use runtime_model::deployment::{Endpoint, ProtocolKind};
use runtime_model::Catalog;
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

/// 真实的设置后端。
struct RealBackend {
    transport: Arc<dyn HttpTransport>,
    /// 模型知识：canonical_id（规范化小写）→ 展示用知识。
    catalog: HashMap<String, UiModelInfo>,
}

impl RealBackend {
    fn new() -> Self {
        let catalog = load_catalog_from_env();
        if catalog.is_empty() {
            eprintln!(
                "[demo] 未加载到模型目录：设置 UMER_DEMO_CATALOG 指向 `model-data build` 的产物，\n\
                 [demo] 例如：set UMER_DEMO_CATALOG=runtime-data\\out\\catalog.json"
            );
        } else {
            eprintln!("[demo] 模型目录已加载：{} 条", catalog.len());
        }
        Self {
            transport: Arc::new(RealHttpTransport::new(HttpConfig::default())),
            catalog,
        }
    }

    fn endpoint_of(draft: &SettingsDraft) -> Result<Endpoint, BackendError> {
        let url = draft.values.get("endpoint").cloned().unwrap_or_default();
        if url.trim().is_empty() {
            return Err(BackendError::new(
                "validation.endpoint.required",
                "endpoint empty",
            ));
        }
        Ok(Endpoint {
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
        match draft.values.get("protocol").map(String::as_str) {
            Some("openai_responses") => ProtocolKind::OpenAiResponses,
            Some("anthropic_messages") => ProtocolKind::AnthropicMessages,
            Some("gemini") => ProtocolKind::Gemini,
            _ => ProtocolKind::OpenAiChat,
        }
    }

    /// 本次会话的凭据（API Key 只存在内存，§32）。
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

    fn adapter_of(
        draft: &SettingsDraft,
        transport: Arc<dyn HttpTransport>,
    ) -> Arc<dyn ProviderAdapter> {
        match Self::protocol_of(draft) {
            ProtocolKind::OpenAiResponses => Arc::new(OpenAiResponsesAdapter::new(transport)),
            ProtocolKind::AnthropicMessages => Arc::new(AnthropicAdapter::new(transport)),
            ProtocolKind::Gemini => Arc::new(GeminiAdapter::new(transport)),
            ProtocolKind::OpenAiChat | ProtocolKind::ProviderNative => {
                Arc::new(OpenAiChatAdapter::new(transport))
            }
        }
    }
}

impl SettingsBackend for RealBackend {
    fn test_connection(&self, draft: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
        let endpoint = Self::endpoint_of(draft)?;
        let credentials = Self::credentials_of(draft);
        let adapter = Self::adapter_of(draft, self.transport.clone());
        let started = std::time::Instant::now();
        // 连接测试 = GET /models（Passive；§17.1）
        adapter
            .discover_models(&endpoint, &credentials, &CredentialRef::from(KEY_REF))
            .map_err(|e| BackendError::new("connection.failed", e.to_string()))?;
        Ok(ConnectionReport {
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    fn discover(&self, draft: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
        let endpoint = Self::endpoint_of(draft)?;
        let credentials = Self::credentials_of(draft);
        let adapter = Self::adapter_of(draft, self.transport.clone());
        let models = adapter
            .discover_models(&endpoint, &credentials, &CredentialRef::from(KEY_REF))
            .map_err(|e| BackendError::new("discovery.no_model_list", e.to_string()))?;
        Ok(models
            .into_iter()
            .map(|m| UiModelEntry {
                model_id: m.model_id,
                display_name: m.display_name,
            })
            .collect())
    }

    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        lookup(&self.catalog, model_id).cloned()
    }

    /// URL 预览直接调用 Adapter 的 URL 函数——与真实请求同源，不会漂移。
    fn preview_request_url(&self, draft: &SettingsDraft) -> Option<String> {
        let endpoint = Self::endpoint_of(draft).ok()?;
        let model_id = draft.values.get("model").cloned().unwrap_or_default();
        Some(match Self::protocol_of(draft) {
            ProtocolKind::OpenAiChat => OpenAiChatAdapter::chat_completions_url(&endpoint),
            ProtocolKind::OpenAiResponses => OpenAiResponsesAdapter::responses_url(&endpoint),
            ProtocolKind::AnthropicMessages => AnthropicAdapter::messages_url(&endpoint),
            ProtocolKind::Gemini => GeminiAdapter::stream_url(&endpoint, &model_id),
            ProtocolKind::ProviderNative => endpoint.url.clone(),
        })
    }
}

/// 目录查找：规范化 + 去 provider 前缀 + 逐段回退匹配。
///
/// 用户的模型 ID 形态很杂（`deepseek-chat` / `deepseek/deepseek-chat` /
/// `deepseek-ai/DeepSeek-V3`），而目录键是规范化的 canonical_id。
fn lookup<'a>(
    catalog: &'a HashMap<String, UiModelInfo>,
    model_id: &str,
) -> Option<&'a UiModelInfo> {
    let normalized = runtime_model::normalize_model_id(model_id);
    if let Some(hit) = catalog.get(&normalized) {
        return Some(hit);
    }
    // 去掉 provider 前缀再试
    if let Some((_, bare)) = normalized.rsplit_once('/') {
        if let Some(hit) = catalog.get(bare) {
            return Some(hit);
        }
    }
    // 后缀匹配：目录键以给定 ID 结尾（处理 `accounts/fireworks/models/xxx` 这类）
    catalog
        .iter()
        .find(|(key, _)| key.ends_with(&normalized) && !normalized.is_empty())
        .map(|(_, value)| value)
}

/// 从 `UMER_DEMO_CATALOG` 指向的构建产物加载模型知识。
fn load_catalog_from_env() -> HashMap<String, UiModelInfo> {
    let Ok(path) = std::env::var("UMER_DEMO_CATALOG") else {
        return HashMap::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("[demo] 无法读取 {path}");
        return HashMap::new();
    };
    // 构建产物形如 { catalog: {...}, records: [...], ... }
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[demo] {path} 解析失败：{e}");
            return HashMap::new();
        }
    };
    let catalog_value = value.get("catalog").cloned().unwrap_or(value);
    let catalog: Catalog = match serde_json::from_value(catalog_value) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[demo] {path} 不是 Canonical Catalog：{e}");
            return HashMap::new();
        }
    };

    let mut map = HashMap::new();
    for profile in &catalog.entries {
        let capabilities: Vec<(CapabilityKind, CapabilityStatus)> = profile
            .capabilities
            .iter()
            .map(|(kind, record)| (*kind, record.status))
            .collect();
        let info = UiModelInfo {
            model_id: profile.identity.canonical_id.clone(),
            display_name: if profile.display_name.is_empty() {
                None
            } else {
                Some(profile.display_name.clone())
            },
            capabilities,
            limits: profile.limits,
            pricing: profile.pricing.clone(),
            supported_efforts: profile.reasoning.supported_efforts.clone(),
            evidence: vec![],
        };
        map.insert(
            runtime_model::normalize_model_id(&profile.identity.canonical_id),
            info.clone(),
        );
        // 别名也进索引，提高命中率（人工审核的 alias 表）
        for alias in &profile.identity.aliases {
            map.entry(runtime_model::normalize_model_id(alias))
                .or_insert_with(|| info.clone());
        }
    }
    map
}

fn main() {
    let backend = RealBackend::new();
    let params = SettingsWindowParams {
        title: "Universal Model Runtime — 模型服务".into(),
        theme: ThemeMode::Dark,
        language: "zh-CN",
        scale: 1.0,
        density: Density::Cozy,
        page: runtime_ui::SettingsPage::provider_settings(),
        backend: Arc::new(backend),
    };
    if let Err(e) = open_settings_window(params) {
        eprintln!("settings window failed: {e}");
        std::process::exit(1);
    }
}
