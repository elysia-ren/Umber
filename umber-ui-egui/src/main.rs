//! settings-demo：设置窗口的完整宿主接线（真实后端）。
//!
//! 数据来源：
//! - **模型知识**：由 `model-data build` 产出的 Canonical Catalog
//!   （实测：6158 个模型，含能力/上下文/价格/档位/证据）。
//!   用环境变量 `UMER_DEMO_CATALOG` 指定路径；未提供时退回"全未知"，
//!   界面会如实显示"该模型无目录数据"而不是编造。
//! - **协议与网络**：`umber-protocol` 的真实适配器。
//! - URL 预览直接调用适配器的 URL 构造函数——**预览与实际请求不可能漂移**。
//!
//! 只做 Passive 动作（GET /models），不消耗 Token（总案 §17.1）。

use std::collections::HashMap;
use std::sync::Arc;

use umber_core::error::{ErrorDetail, ModelError};
use umber_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use umber_credential_os::{CredentialTier, EncryptedFileStore, FallbackChain, OsKeystore};
use umber_data::{LocalDb, StoredDeployment, UserOverride};
use umber_model::capability::{CapabilityKind, CapabilityStatus};
use umber_model::deployment::{Endpoint, ProtocolKind};
use umber_model::Catalog;
use umber_protocol::{
    AnthropicAdapter, GeminiAdapter, HttpConfig, HttpTransport, OpenAiChatAdapter,
    OpenAiResponsesAdapter, RealHttpTransport,
};
use umber_provider::{DiscoveredModel, ProviderAdapter};
use umber_ui::{
    BackendError, ConnectionReport, SaveReport, SavedSettings, SettingsBackend, SettingsDraft,
    UiModelEntry, UiModelInfo,
};
use umber_ui_egui::{open_settings_window, Density, SettingsWindowParams, ThemeMode};

const KEY_REF: &str = "settings/api_key";
/// `--selftest` 专用：与正式配置**完全隔离**的凭据引用与钥匙串服务名。
/// 自检不得读到 / 覆盖 / 删除用户已保存的密钥。
const SELFTEST_KEY_REF: &str = "selftest/api_key";
const SELFTEST_SERVICE: &str = "UmberTest";

/// 取 URL 的 `scheme://host[:port]`。模型发现的回退候选要用它——
/// 不引入 url crate，只按第一个 `/` 切。
fn origin_of(url: &str) -> Option<String> {
    let sep = url.find("://")?;
    let rest = &url[sep + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() {
        return None;
    }
    Some(format!("{}://{}", &url[..sep], host))
}

/// 这个错误是否表示"主机可达、只是这家服务没有模型列表端点"。
///
/// **"没有模型列表" ≠ "连不上"**：模型列表是厂商级的可选能力（§36 Discovery
/// 回退：无 `/models` 不阻断）。请求已到达服务端并按路径被拒（404 / 不支持），
/// 说明网络与鉴权都过了；真正该判失败的只有认证 / 限流 / 超时 / 网络错误。
fn means_no_model_list(err: &ModelError) -> bool {
    matches!(
        err,
        ModelError::InvalidRequest(_)
            | ModelError::Unsupported(_)
            | ModelError::ProviderError {
                status: Some(404 | 405 | 501),
                ..
            }
    )
}

/// 模型发现失败的原因。分开是为了让连接测试能区分
/// "配置不全" / "所有候选都拿不到模型列表"，而后者里还要再分
/// "服务没有这个端点（不算失败）" 与 "认证/网络失败（算失败）"。
enum DiscoverError {
    Config(BackendError),
    NoModelList(ModelError),
}

/// 真实的设置后端。
struct RealBackend {
    transport: Arc<dyn HttpTransport>,
    /// 模型知识：canonical_id（规范化小写）→ 展示用知识。
    catalog: HashMap<String, UiModelInfo>,
    /// provider → 该 provider 暴露的模型 ID（规格 X.18 的 Deployment 表）。
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
        return std::path::PathBuf::from(appdata).join("Umber");
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("umber");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".local/share/umber")
}

impl StoreBundle {
    fn open() -> Option<Self> {
        Self::open_in(&data_dir(), None, KEY_REF)
    }

    /// `service = None` → 正式钥匙串服务名；`Some` 用于 `--selftest` 隔离。
    fn open_in(dir: &std::path::Path, service: Option<&str>, reference: &str) -> Option<Self> {
        let db = LocalDb::open(dir).ok()?;
        let credentials_dir = dir.join("credentials");
        let mut tiers: Vec<(CredentialTier, Arc<dyn CredentialStore>)> = Vec::new();
        // 回退顺序（§32.1）：宿主实现 → 系统钥匙串 → 加密文件
        let keystore = match service {
            Some(name) => OsKeystore::with_service(name),
            None => OsKeystore::new(),
        };
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
            credential_ref: CredentialRef::from(reference),
        })
    }
}

impl RealBackend {
    fn new() -> Self {
        Self::build(StoreBundle::open())
    }

    /// `--selftest` 用：换成隔离存储，其余（目录 / 传输）与正式启动一致。
    fn with_store(store: StoreBundle) -> Self {
        Self::build(Some(store))
    }

    fn build(store: Option<StoreBundle>) -> Self {
        let catalog = load_catalog_from_env();
        if catalog.is_empty() {
            eprintln!(
                "[demo] 未加载到模型目录：设置 UMER_DEMO_CATALOG 指向 `model-data build` 的产物，\n\
                 [demo] 例如：set UMER_DEMO_CATALOG=umber-data\\out\\catalog.json"
            );
        } else {
            eprintln!("[demo] 模型目录已加载：{} 条", catalog.len());
        }
        Self {
            transport: Arc::new(RealHttpTransport::new(HttpConfig::default())),
            catalog,
            provider_models: load_provider_models_from_env(),
            store: std::sync::Mutex::new(store),
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

    /// 本次会话的凭据：优先用用户刚输入的 `api_key`，否则回落到已保存的（§32）。
    ///
    /// 只读回内存供本次调用使用，密钥不进入任何配置文件。
    fn credentials_for(&self, api_key: Option<&str>) -> InMemoryCredentialStore {
        let store = InMemoryCredentialStore::new();
        let provided = api_key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_string);
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
    }

    /// 协议 → 适配器：全项目只留这一处映射。
    fn adapter_for(
        kind: ProtocolKind,
        transport: Arc<dyn HttpTransport>,
    ) -> Arc<dyn ProviderAdapter> {
        match kind {
            ProtocolKind::OpenAiResponses => Arc::new(OpenAiResponsesAdapter::new(transport)),
            ProtocolKind::AnthropicMessages => Arc::new(AnthropicAdapter::new(transport)),
            ProtocolKind::Gemini => Arc::new(GeminiAdapter::new(transport)),
            ProtocolKind::OpenAiChat | ProtocolKind::ProviderNative => {
                Arc::new(OpenAiChatAdapter::new(transport))
            }
        }
    }
}

impl RealBackend {
    /// 模型发现候选：(协议, 端点)，按尝试顺序。
    ///
    /// **模型列表是厂商级能力，不是协议级能力**：多数厂商的 Anthropic / Gemini
    /// 兼容层只实现对话端点。例如 DeepSeek 官方端点支持表里 Anthropic 兼容只有
    /// `/messages`，没有 `/models`；而模型列表在同一主机的 OpenAI 兼容端点
    /// `/v1/models` 上。只用「当前协议 + /models」判断连通性，会把完全可用的
    /// 配置报成"连接失败"。
    ///
    /// 顺序：当前协议自己的端点 → 厂商预置里的 OpenAI 兼容端点 →
    /// 同源 `{origin}/v1` → 同源 `{origin}`。
    fn discovery_candidates(
        draft: &SettingsDraft,
        endpoint: &Endpoint,
    ) -> Vec<(ProtocolKind, Endpoint)> {
        let mut out: Vec<(ProtocolKind, Endpoint)> = Vec::new();
        let mut push = |kind: ProtocolKind, url: &str| {
            let url = url.trim();
            if url.is_empty() || out.iter().any(|(_, e)| e.url == url) {
                return;
            }
            out.push((
                kind,
                Endpoint {
                    id: endpoint.id.clone(),
                    provider_id: endpoint.provider_id.clone(),
                    url: url.to_string(),
                },
            ));
        };

        push(Self::protocol_of(draft), &endpoint.url);

        let provider_id = draft.values.get("provider").cloned().unwrap_or_default();
        if let Some(preset) = umber_ui::preset_by_id(&provider_id) {
            for offering in preset.offerings {
                if offering.protocol == ProtocolKind::OpenAiChat {
                    push(ProtocolKind::OpenAiChat, offering.default_endpoint);
                }
            }
        }

        if let Some(origin) = origin_of(&endpoint.url) {
            push(ProtocolKind::OpenAiChat, &format!("{origin}/v1"));
            push(ProtocolKind::OpenAiChat, &origin);
        }

        out
    }

    /// 按 (协议, 端点) 取模型列表。
    fn discover_with(
        &self,
        kind: ProtocolKind,
        endpoint: &Endpoint,
        credentials: &InMemoryCredentialStore,
    ) -> Result<Vec<DiscoveredModel>, ModelError> {
        Self::adapter_for(kind, self.transport.clone()).discover_models(
            endpoint,
            credentials,
            &CredentialRef::from(KEY_REF),
        )
    }

    /// 按候选顺序发现模型，第一个成功即返回；全失败时返回**首个**错误
    /// （即用户所选配置的报错，最相关）。
    fn discover_any(
        &self,
        draft: &SettingsDraft,
        credentials: &InMemoryCredentialStore,
    ) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        let endpoint = Self::endpoint_of(draft).map_err(DiscoverError::Config)?;
        let mut first: Option<ModelError> = None;
        for (kind, candidate) in Self::discovery_candidates(draft, &endpoint) {
            match self.discover_with(kind, &candidate, credentials) {
                Ok(models) => return Ok(models),
                Err(err) => {
                    if first.is_none() {
                        first = Some(err);
                    }
                }
            }
        }
        Err(DiscoverError::NoModelList(first.unwrap_or_else(|| {
            ModelError::Unknown(ErrorDetail::new("no discovery endpoint candidate"))
        })))
    }
}

impl SettingsBackend for RealBackend {
    fn test_connection(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<ConnectionReport, BackendError> {
        // 关键：用户刚输入的密钥必须送到这里，否则请求没带 key，
        // 只会拿到 provider 的 "Authentication Fails"
        let credentials = self.credentials_for(api_key);
        let started = std::time::Instant::now();
        // 连接测试 = GET /models（Passive；§17.1）。
        // 但**拿不到模型列表不等于连不上**：见 discovery_candidates 与
        // means_no_model_list 的说明。
        match self.discover_any(draft, &credentials) {
            Ok(_) => {}
            Err(DiscoverError::Config(err)) => return Err(err),
            Err(DiscoverError::NoModelList(err)) => {
                if !means_no_model_list(&err) {
                    return Err(BackendError::new("connection.failed", err.to_string()));
                }
            }
        }
        Ok(ConnectionReport {
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    fn discover(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<Vec<UiModelEntry>, BackendError> {
        let credentials = self.credentials_for(api_key);
        let models = match self.discover_any(draft, &credentials) {
            Ok(models) => models,
            Err(DiscoverError::Config(err)) => return Err(err),
            Err(DiscoverError::NoModelList(err)) => {
                return Err(BackendError::new(
                    "discovery.no_model_list",
                    err.to_string(),
                ))
            }
        };
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

    /// 按厂商**查随包目录**取推荐模型（不是硬编码模型名）。
    ///
    /// 目录里没有该厂商（如百度千帆尚未收录）时返回空，
    /// 界面会提示"刷新模型列表"或让用户手填，而不是显示过时型号。
    fn recommend_models(&self, catalog_provider_ids: &[String], limit: usize) -> Vec<UiModelEntry> {
        if catalog_provider_ids.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut ids: Vec<String> = Vec::new();
        for provider in catalog_provider_ids {
            if let Some(models) = self.provider_models.get(&provider.to_lowercase()) {
                ids.extend(models.iter().cloned());
            }
        }
        ids.sort();
        ids.dedup();

        // 排序：上下文窗口大的在前（通常更强/更新），再按名字，保证稳定
        ids.sort_by(|a, b| {
            let ctx = |id: &str| {
                crate::lookup(&self.catalog, id)
                    .and_then(|info| info.limits.context_window)
                    .unwrap_or(0)
            };
            ctx(b).cmp(&ctx(a)).then_with(|| a.cmp(b))
        });

        ids.into_iter()
            .take(limit)
            .map(|model_id| {
                let display_name = crate::lookup(&self.catalog, &model_id)
                    .and_then(|info| info.display_name.clone());
                UiModelEntry {
                    model_id,
                    display_name,
                }
            })
            .collect()
    }

    /// 读上次保存的配置。密钥只回"有没有"，不回值（§32）。
    fn load_settings(&self) -> Option<SavedSettings> {
        let guard = self.store.lock().ok()?;
        let bundle = guard.as_ref()?;
        let deployments = bundle.db.deployments().ok()?;
        let deployment = deployments.iter().max_by_key(|d| d.created_at_unix)?;
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
    let normalized = umber_model::normalize_model_id(model_id);
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
            provider: profile.identity.organization.clone(),
            capabilities,
            limits: profile.limits,
            pricing: profile.pricing.clone(),
            supported_efforts: profile.reasoning.supported_efforts.clone(),
            evidence: vec![],
        };
        map.insert(
            umber_model::normalize_model_id(&profile.identity.canonical_id),
            info.clone(),
        );
        // 别名也进索引，提高命中率（人工审核的 alias 表）
        for alias in &profile.identity.aliases {
            map.entry(umber_model::normalize_model_id(alias))
                .or_insert_with(|| info.clone());
        }
    }
    map
}

/// 从构建产物加载 provider → 模型 的归属表。
fn load_provider_models_from_env() -> HashMap<String, Vec<String>> {
    let Ok(path) = std::env::var("UMER_DEMO_CATALOG") else {
        return HashMap::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    let catalog_value = value.get("catalog").cloned().unwrap_or(value);
    let Ok(catalog) = serde_json::from_value::<Catalog>(catalog_value) else {
        return HashMap::new();
    };
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for deployment in &catalog.deployments {
        index
            .entry(deployment.provider.to_lowercase())
            .or_default()
            .push(deployment.model_id.clone());
    }
    index
}

/// `--selftest`：不开窗口，直接用真实 Local DB + 凭据回退链走一遍
/// 保存 → 重新读取，把结果打印出来。
///
/// 这是"保存到底有没有落盘"的可验证证据——不依赖点击界面。
fn selftest() -> i32 {
    // 隔离：独立数据子目录 + 独立凭据引用 + 独立钥匙串服务名。
    // 此前自检与正式界面共用 `settings/api_key` 与同一个服务名，跑一次就会
    // 覆盖并删除用户已保存的密钥；现在自检既读不到也删不掉用户的数据。
    let dir = data_dir().join("selftest");
    let Some(store) = StoreBundle::open_in(&dir, Some(SELFTEST_SERVICE), SELFTEST_KEY_REF) else {
        println!("[selftest] FAIL: 无法打开隔离存储 {}", dir.display());
        return 1;
    };
    let backend = RealBackend::with_store(store);
    let mut draft = SettingsDraft::default();
    draft.values.insert("provider".into(), "selftest".into());
    draft.values.insert("protocol".into(), "openai_chat".into());
    draft
        .values
        .insert("endpoint".into(), "https://selftest.example/v1".into());
    draft.values.insert("model".into(), "selftest-model".into());
    draft
        .values
        .insert("context_window".into(), "123456".into());

    println!("[selftest] 数据目录: {}", data_dir().display());
    match backend.save_settings(&draft, "sk-selftest-only") {
        Ok(report) => println!(
            "[selftest] 保存: config={} credential={} warning={:?}",
            report.saved_config, report.saved_credential, report.credential_warning_key
        ),
        Err(e) => {
            println!("[selftest] 保存失败: {} ({})", e.reason_key, e.detail);
            return 1;
        }
    }

    match backend.load_settings() {
        Some(saved) => {
            println!(
                "[selftest] 读回: provider={} protocol={} endpoint={} model={:?} ctx={:?} has_key={}",
                saved.provider,
                saved.protocol,
                saved.endpoint,
                saved.model_id,
                saved.context_window,
                saved.has_api_key
            );
            if saved.endpoint != "https://selftest.example/v1"
                || saved.model_id.as_deref() != Some("selftest-model")
                || saved.context_window != Some(123_456)
                || !saved.has_api_key
            {
                println!("[selftest] FAIL: 读回值与保存值不一致");
                return 1;
            }
        }
        None => {
            println!("[selftest] FAIL: 读回为空");
            return 1;
        }
    }

    // 清理：删凭据 + 删整个隔离目录（deployment / override / 加密文件一并消失）
    let credential_removed = match backend.store.lock() {
        Ok(guard) => guard
            .as_ref()
            .map(|b| b.credentials.delete(&b.credential_ref).unwrap_or(false))
            .unwrap_or(false),
        Err(_) => false,
    };
    // 先释放 LocalDb / 加密文件句柄，再删目录（Windows 上句柄会挡住删除）
    drop(backend);
    let dir_removed = std::fs::remove_dir_all(&dir).is_ok() || !dir.exists();
    println!("[selftest] 清理: credential={credential_removed} dir={dir_removed}");
    if !dir_removed {
        println!("[selftest] 提示: 隔离目录未删净 {}", dir.display());
    }
    println!("[selftest] PASS（自检使用隔离存储，用户配置与密钥未被触碰）");
    0
}

fn main() {
    if std::env::args().any(|arg| arg == "--selftest") {
        std::process::exit(selftest());
    }
    let backend = RealBackend::new();
    let params = SettingsWindowParams {
        title: "Universal Model Runtime — 模型服务".into(),
        theme: ThemeMode::Dark,
        language: "zh-CN",
        scale: 1.0,
        density: Density::Cozy,
        page: umber_ui::SettingsPage::provider_settings(),
        backend: Arc::new(backend),
    };
    if let Err(e) = open_settings_window(params) {
        eprintln!("settings window failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_for(provider: &str, protocol: &str, endpoint: &str) -> SettingsDraft {
        let mut draft = SettingsDraft::default();
        draft.values.insert("provider".into(), provider.into());
        draft.values.insert("protocol".into(), protocol.into());
        draft.values.insert("endpoint".into(), endpoint.into());
        draft
    }

    fn endpoint_of(url: &str) -> Endpoint {
        Endpoint {
            id: "ep".into(),
            provider_id: "p".into(),
            url: url.into(),
        }
    }

    #[test]
    fn origin_of_keeps_scheme_host_and_port() {
        assert_eq!(
            origin_of("https://api.deepseek.com/anthropic").as_deref(),
            Some("https://api.deepseek.com")
        );
        assert_eq!(
            origin_of("http://localhost:11434/v1").as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(
            origin_of("https://api.deepseek.com").as_deref(),
            Some("https://api.deepseek.com")
        );
        assert_eq!(origin_of("not-a-url"), None);
    }

    /// 这是本 bug 的回归测试：Anthropic 协议的 DeepSeek 必须回退到
    /// 厂商的 OpenAI 兼容端点去取模型列表，否则连接测试会误报失败。
    #[test]
    fn discovery_falls_back_to_the_vendors_openai_endpoint() {
        let draft = draft_for(
            "deepseek",
            "anthropic_messages",
            "https://api.deepseek.com/anthropic",
        );
        let candidates =
            RealBackend::discovery_candidates(&draft, &endpoint_of(&draft.values["endpoint"]));
        assert_eq!(candidates[0].0, ProtocolKind::AnthropicMessages);
        assert_eq!(candidates[0].1.url, "https://api.deepseek.com/anthropic");
        assert!(
            candidates
                .iter()
                .any(|(k, e)| *k == ProtocolKind::OpenAiChat
                    && e.url == "https://api.deepseek.com/v1"),
            "必须包含厂商预置里的 OpenAI 兼容端点"
        );
        let urls: Vec<&str> = candidates.iter().map(|(_, e)| e.url.as_str()).collect();
        let mut deduped = urls.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(urls.len(), deduped.len(), "候选不得重复");
    }

    #[test]
    fn missing_model_list_is_not_a_connection_failure() {
        assert!(means_no_model_list(&ModelError::InvalidRequest(
            ErrorDetail::new("404")
        )));
        assert!(means_no_model_list(&ModelError::Unsupported(
            ErrorDetail::new("no /models")
        )));
        assert!(means_no_model_list(&ModelError::ProviderError {
            status: Some(501),
            detail: ErrorDetail::new("not implemented"),
        }));
        assert!(!means_no_model_list(&ModelError::AuthenticationFailed(
            ErrorDetail::new("bad key")
        )));
        assert!(!means_no_model_list(&ModelError::NetworkError(
            ErrorDetail::new("dns")
        )));
        assert!(!means_no_model_list(&ModelError::Timeout {
            kind: umber_core::error::TimeoutKind::Connect,
            detail: ErrorDetail::new("t"),
        }));
    }
}
