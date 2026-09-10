//! 设置界面的状态（替代线性向导）。
//!
//! 结构对齐现实产品的用法：
//!
//! ```text
//! 左侧：厂商列表（可搜索、按类分组、带状态点）
//! 右侧：当前厂商的配置（协议 / 地址+实时预览 / 密钥 / 模型列表 / 上下文证据）
//! ```
//!
//! **纯逻辑、无 GUI 依赖**，因此"选了厂商带没带出地址""上下文该取哪个值"
//! 这类问题能在无 GUI 测试里查到。

use runtime_core::request::ReasoningEffort;
use runtime_model::deployment::ProtocolKind;
use runtime_model::evidence::EvidenceSource;
use runtime_model::resolver::{self, FieldCandidate, FieldCategory, FieldValue};

use crate::backend::UiModelInfo;
use crate::preset::{preset_by_id, search_presets_localized, ProviderCategory, ProviderPreset};
use crate::schema::SettingsDraft;

/// 模型条目的来源。界面需要区分"目录推荐"与"从服务实时拉到"，
/// 刷新时也只替换对应来源的条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// 随包目录里按厂商匹配到的（不是硬编码，而是查目录得到）。
    Catalog,
    /// 从服务商 `/models` 实时拉取的。
    Discovered,
    /// 用户手填的。
    Manual,
}

impl ModelSource {
    pub fn label_key(self) -> &'static str {
        match self {
            ModelSource::Catalog => "model.source.catalog",
            ModelSource::Discovered => "model.source.discovered",
            ModelSource::Manual => "model.source.manual",
        }
    }
}

/// 一行的模型条目（右侧模型列表用）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    pub model_id: String,
    pub display_name: Option<String>,
    /// 已知的模型知识；`None` 表示"目录里没有这个模型"（仍可正常使用）。
    pub profile: Option<UiModelInfo>,
    /// 这个条目从哪来。
    pub source: ModelSource,
}

impl ModelEntry {
    pub fn new(model_id: impl Into<String>, profile: Option<UiModelInfo>) -> Self {
        Self::with_source(model_id, profile, ModelSource::Manual)
    }

    pub fn with_source(
        model_id: impl Into<String>,
        profile: Option<UiModelInfo>,
        source: ModelSource,
    ) -> Self {
        let model_id = model_id.into();
        Self {
            display_name: profile.as_ref().and_then(|p| p.display_name.clone()),
            model_id,
            profile,
            source,
        }
    }

    pub fn label(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| self.model_id.clone())
    }
}

/// 上下文窗口的证据解析结果（把 Runtime 的 Evidence 架构直接呈现给用户）。
///
/// 对应参考产品那行 `自动探测 256,000 · 覆盖 1,024,000 · 生效 1,024,000`——
/// 我们的 Resolver 本来就在做这件事，只是之前没露出来。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextWindowEvidence {
    /// 目录/上游声明的值（Bundled Catalog）。
    pub catalog: Option<u64>,
    /// 自动探测到的值（Passive Probe）。
    pub probed: Option<u64>,
    /// 用户显式覆盖的值。
    pub overridden: Option<u64>,
    /// 最终生效值。
    pub effective: Option<u64>,
    /// 生效值来自哪个来源（"catalog" / "probe" / "user"）。
    pub source: Option<&'static str>,
    /// 各来源之间是否存在冲突（用户需要知道"我覆盖掉了别的值"）。
    pub conflict: bool,
}

impl ContextWindowEvidence {
    pub fn has_any(&self) -> bool {
        self.effective.is_some()
    }
}

/// 用 Resolver 解析上下文窗口：用户覆盖 > 官方/目录 > 探测（§13 字段级优先级）。
pub fn resolve_context_window(
    catalog: Option<u64>,
    probed: Option<u64>,
    overridden: Option<u64>,
) -> ContextWindowEvidence {
    let mut candidates: Vec<FieldCandidate> = Vec::new();
    if let Some(v) = catalog {
        candidates.push(FieldCandidate::new(
            FieldValue::U64(v),
            EvidenceSource::BundledCatalog,
        ));
    }
    if let Some(v) = probed {
        candidates.push(FieldCandidate::new(
            FieldValue::U64(v),
            EvidenceSource::Probe { tested_at_unix: 0 },
        ));
    }
    if let Some(v) = overridden {
        candidates.push(FieldCandidate::new(
            FieldValue::U64(v),
            EvidenceSource::User,
        ));
    }

    let resolution = resolver::resolve(FieldCategory::Spec, &candidates);
    let (effective, source, conflict) = match &resolution {
        Some(r) => {
            let source = match &r.winner {
                EvidenceSource::User => "user",
                EvidenceSource::Probe { .. } => "probe",
                EvidenceSource::BundledCatalog => "catalog",
                EvidenceSource::OfficialDocs => "official",
                EvidenceSource::ProviderApi => "provider_api",
                EvidenceSource::ThirdPartyCatalog { .. } => "third_party",
            };
            (
                match r.value {
                    FieldValue::U64(v) => Some(v),
                    _ => None,
                },
                Some(source),
                r.conflict,
            )
        }
        None => (None, None, false),
    };

    ContextWindowEvidence {
        catalog,
        probed,
        overridden,
        effective,
        source,
        conflict,
    }
}

/// 设置界面的完整状态。
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// 厂商搜索词。
    pub search: String,
    selected_provider: String,
    protocol: ProtocolKind,
    endpoint: String,
    api_key: String,
    /// 用户手工改过地址后，Preset 不再覆盖（§34）。
    endpoint_touched: bool,
    models: Vec<ModelEntry>,
    selected_model: Option<String>,
    /// 用户对上下文窗口的覆盖（空串 = 不覆盖）。
    context_override_input: String,
    /// 自动探测值（由 Probe 写入；未探测为 None）。
    probed_context: Option<u64>,
    /// 模型发现的结果计数（用于显示"已获取 N 个"）。
    discovered_count: usize,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsState {
    pub fn new() -> Self {
        let mut state = Self {
            search: String::new(),
            selected_provider: String::new(),
            protocol: ProtocolKind::OpenAiChat,
            endpoint: String::new(),
            api_key: String::new(),
            endpoint_touched: false,
            models: Vec::new(),
            selected_model: None,
            context_override_input: String::new(),
            probed_context: None,
            discovered_count: 0,
        };
        // 默认落在 DeepSeek：国内用户最常见的选择
        let initial = if preset_by_id("deepseek").is_some() {
            "deepseek"
        } else {
            "custom"
        };
        state.select_provider(initial);
        state
    }

    // ---------- 厂商列表 ----------

    pub fn preset(&self) -> &'static ProviderPreset {
        preset_by_id(&self.selected_provider)
            .unwrap_or_else(|| preset_by_id("custom").expect("custom preset must exist"))
    }

    pub fn selected_provider_id(&self) -> &str {
        &self.selected_provider
    }

    /// 过滤后的厂商（按本地化名称搜索）。空搜索返回全部，保持原顺序。
    pub fn filtered_presets<F>(&self, name_of: F) -> Vec<&'static ProviderPreset>
    where
        F: Fn(&ProviderPreset) -> String,
    {
        search_presets_localized(&self.search, name_of)
    }

    /// 按分类分组（分组顺序固定）。
    pub fn grouped<F>(&self, name_of: F) -> Vec<(ProviderCategory, Vec<&'static ProviderPreset>)>
    where
        F: Fn(&ProviderPreset) -> String + Copy,
    {
        let filtered: Vec<&'static ProviderPreset> = self.filtered_presets(name_of);
        crate::preset::presets_by_category()
            .into_iter()
            .map(|(category, _)| {
                let items: Vec<&'static ProviderPreset> = filtered
                    .iter()
                    .copied()
                    .filter(|p| p.category == category)
                    .collect();
                (category, items)
            })
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }

    /// 选厂商：**带出该厂商的默认协议与默认地址**，并清空模型列表
    /// （上一个厂商的模型不属于这个厂商）。
    pub fn select_provider(&mut self, preset_id: &str) {
        let Some(preset) = preset_by_id(preset_id) else {
            return;
        };
        self.selected_provider = preset.id.to_string();
        let first = &preset.offerings[0];
        self.protocol = first.protocol;
        self.endpoint = first.default_endpoint.to_string();
        self.endpoint_touched = false;
        self.models.clear();
        self.selected_model = None;
        self.context_override_input.clear();
        self.probed_context = None;
        self.discovered_count = 0;
        // 注意：这里**不注入任何模型名**。推荐模型由宿主按厂商查随包目录后
        // 经 `apply_recommendations` 写入，实时清单由 `apply_discovery` 写入。
        // 早期版本在这里写死了模型名（如 moonshot-v1-8k），用户看到的是过时型号。
    }

    // ---------- 连接配置 ----------

    pub fn protocol(&self) -> ProtocolKind {
        self.protocol
    }

    pub fn set_protocol(&mut self, protocol: ProtocolKind) {
        let Some(offering) = self.preset().offering(protocol) else {
            return; // 该厂商不支持此协议：忽略，不产生非法组合
        };
        self.protocol = protocol;
        if !self.endpoint_touched || self.endpoint.trim().is_empty() {
            self.endpoint = offering.default_endpoint.to_string();
        }
        // 同样不注入模型名：换协议后界面会重新向宿主请求推荐
        let _ = offering;
    }

    pub fn available_protocols(&self) -> Vec<ProtocolKind> {
        self.preset().offerings.iter().map(|o| o.protocol).collect()
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn set_endpoint(&mut self, endpoint: impl Into<String>) {
        self.endpoint = endpoint.into();
        self.endpoint_touched = true;
    }

    pub fn reset_endpoint(&mut self) {
        if let Some(offering) = self.preset().offering(self.protocol) {
            self.endpoint = offering.default_endpoint.to_string();
            self.endpoint_touched = false;
        }
    }

    pub fn endpoint_is_default(&self) -> bool {
        self.preset()
            .offering(self.protocol)
            .map(|o| o.default_endpoint == self.endpoint)
            .unwrap_or(false)
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn set_api_key(&mut self, key: impl Into<String>) {
        self.api_key = key.into();
    }

    /// 该厂商是否无需密钥。
    pub fn keyless(&self) -> bool {
        self.preset().keyless
    }

    // ---------- 模型列表 ----------

    pub fn models(&self) -> &[ModelEntry] {
        &self.models
    }

    pub fn selected_model(&self) -> Option<&str> {
        self.selected_model.as_deref()
    }

    pub fn select_model(&mut self, model_id: impl Into<String>) {
        let model_id = model_id.into();
        if !self.models.iter().any(|m| m.model_id == model_id) {
            self.models.push(ModelEntry::new(model_id.clone(), None));
        }
        self.selected_model = Some(model_id);
        // 换模型后探测值失效（不同模型上下文不同）
        self.probed_context = None;
        self.context_override_input.clear();
    }

    pub fn add_model(&mut self, entry: ModelEntry) {
        if let Some(existing) = self
            .models
            .iter_mut()
            .find(|m| m.model_id == entry.model_id)
        {
            // 已存在：合并知识（谁更完整用谁），并保留更强的来源
            let source = if entry.profile.is_some() {
                entry.source
            } else {
                existing.source
            };
            let merged = ModelEntry {
                model_id: entry.model_id,
                display_name: entry.display_name.or_else(|| existing.display_name.clone()),
                profile: entry.profile.or_else(|| existing.profile.clone()),
                source,
            };
            *existing = merged;
            return;
        }
        self.models.push(entry);
    }

    /// 写入**按厂商从目录取到的推荐模型**（宿主在切换厂商/协议后调用）。
    ///
    /// 只替换来自目录的条目：用户已从服务拉到的、手填的一律保留。
    pub fn apply_recommendations(&mut self, recommended: Vec<ModelEntry>) {
        self.models.retain(|m| m.source != ModelSource::Catalog);
        for mut entry in recommended {
            entry.source = ModelSource::Catalog;
            self.add_model(entry);
        }
        if self.selected_model.is_none() {
            if let Some(first) = self.models.first() {
                self.selected_model = Some(first.model_id.clone());
            }
        }
    }

    pub fn remove_model(&mut self, model_id: &str) {
        self.models.retain(|m| m.model_id != model_id);
        if self.selected_model.as_deref() == Some(model_id) {
            self.selected_model = None;
            self.probed_context = None;
        }
    }

    pub fn discovered_count(&self) -> usize {
        self.discovered_count
    }

    /// 发现结果替换列表（推荐模型保留在末尾，避免用户刚选的模型消失）。
    pub fn apply_discovery(&mut self, found: Vec<ModelEntry>) {
        self.discovered_count = found.len();
        // 上一轮实时结果先清掉（服务端清单可能已变），目录推荐与手填保留
        self.models.retain(|m| m.source != ModelSource::Discovered);
        for mut entry in found {
            entry.source = ModelSource::Discovered;
            self.add_model(entry);
        }
        if self.selected_model.is_none() {
            if let Some(first) = self.models.first() {
                self.selected_model = Some(first.model_id.clone());
            }
        }
    }

    /// 当前选中模型的已知知识。
    pub fn selected_profile(&self) -> Option<&UiModelInfo> {
        let selected = self.selected_model.as_deref()?;
        self.models
            .iter()
            .find(|m| m.model_id == selected)
            .and_then(|m| m.profile.as_ref())
    }

    // ---------- 上下文窗口与证据 ----------

    pub fn context_override_input(&self) -> &str {
        &self.context_override_input
    }

    pub fn set_context_override_input(&mut self, text: impl Into<String>) {
        self.context_override_input = text.into();
    }

    /// 用户覆盖值（非数字或不填 = 不覆盖）。
    pub fn context_override(&self) -> Option<u64> {
        let trimmed = self.context_override_input.trim();
        if trimmed.is_empty() || trimmed == "0" {
            return None;
        }
        trimmed.parse::<u64>().ok()
    }

    /// 目录声明的上下文（来自当前选中模型的已知知识）。
    pub fn context_catalog(&self) -> Option<u64> {
        self.selected_profile()
            .and_then(|p| p.limits.context_window)
    }

    pub fn set_probed_context(&mut self, value: Option<u64>) {
        self.probed_context = value;
    }

    /// 上下文窗口的证据解析（界面那行"探测 / 目录 / 覆盖 / 生效"）。
    pub fn context_evidence(&self) -> ContextWindowEvidence {
        resolve_context_window(
            self.context_catalog(),
            self.probed_context,
            self.context_override(),
        )
    }

    // ---------- 能力与档位（展示我们的模型数据优势） ----------

    /// 当前模型实际可用的思考强度档位（空 = 未知，界面须如实说明）。
    pub fn supported_efforts(&self) -> &[ReasoningEffort] {
        self.selected_profile()
            .map(|p| p.supported_efforts.as_slice())
            .unwrap_or(&[])
    }

    /// 请求档位在该模型上的实际生效档位（就近降级，§21.1）。
    pub fn resolve_effort(
        &self,
        requested: ReasoningEffort,
    ) -> Option<runtime_model::effort::EffortResolution> {
        runtime_model::effort::nearest(requested, self.supported_efforts())
    }

    // ---------- 校验与导出 ----------

    pub fn validate(&self) -> Vec<&'static str> {
        let mut issues = Vec::new();
        if self.endpoint.trim().is_empty() {
            issues.push("validation.endpoint.required");
        } else if !(self.endpoint.starts_with("http://") || self.endpoint.starts_with("https://")) {
            issues.push("validation.endpoint.scheme");
        }
        if !self.keyless() && self.api_key.trim().is_empty() {
            // 允许为空（用户可能先存配置后填 Key），因此只作为提示而非阻断
            issues.push("validation.key.empty_hint");
        }
        if self.selected_model.is_none() {
            issues.push("validation.model.required");
        }
        issues
    }

    /// 阻断性问题（决定"保存"按钮是否可用）。
    pub fn blocking_issues(&self) -> Vec<&'static str> {
        self.validate()
            .into_iter()
            .filter(|i| *i != "validation.key.empty_hint")
            .collect()
    }

    pub fn is_ready(&self) -> bool {
        self.blocking_issues().is_empty()
    }

    /// 导出为 Core 的 `SettingsDraft`（凭据不入配置，§32）。
    pub fn to_draft(&self) -> SettingsDraft {
        let mut draft = SettingsDraft::default();
        draft
            .values
            .insert("provider".into(), self.selected_provider.clone());
        draft
            .values
            .insert("protocol".into(), protocol_slug(self.protocol).into());
        draft
            .values
            .insert("endpoint".into(), self.endpoint.clone());
        if let Some(model) = &self.selected_model {
            draft.values.insert("model".into(), model.clone());
        }
        if let Some(context) = self.context_override() {
            draft
                .values
                .insert("context_window".into(), context.to_string());
        }
        draft
    }
}

/// 协议 → 稳定字符串（写进 draft / 存盘）。
pub fn protocol_slug(protocol: ProtocolKind) -> &'static str {
    match protocol {
        ProtocolKind::OpenAiChat => "openai_chat",
        ProtocolKind::OpenAiResponses => "openai_responses",
        ProtocolKind::AnthropicMessages => "anthropic_messages",
        ProtocolKind::Gemini => "gemini",
        ProtocolKind::ProviderNative => "provider_native",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::UiModelInfo;
    use runtime_model::capability::{CapabilityKind, CapabilityStatus};
    use runtime_model::model::ModelLimits;

    fn profile_with(context: Option<u64>, efforts: Vec<ReasoningEffort>) -> UiModelInfo {
        UiModelInfo {
            model_id: "m".into(),
            display_name: Some("M".into()),
            provider: None,
            capabilities: vec![(CapabilityKind::ToolCall, CapabilityStatus::Supported)],
            limits: ModelLimits {
                context_window: context,
                max_output_tokens: None,
            },
            pricing: None,
            supported_efforts: efforts,
            evidence: vec![],
        }
    }

    #[test]
    fn selecting_a_provider_brings_out_protocol_and_endpoint_only() {
        let mut state = SettingsState::new();
        state.select_provider("dashscope");
        assert_eq!(
            state.endpoint(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(state.protocol(), ProtocolKind::OpenAiChat);
        // 关键契约：预置**不得**注入模型名。模型只能来自
        // ① 宿主按厂商查目录 ② 服务 /models 实时拉取 ③ 用户手填。
        assert!(
            state.models().is_empty(),
            "预置不该塞模型名（否则会出现一年前的型号）"
        );
    }

    #[test]
    fn recommendations_from_catalog_are_marked_and_replaceable() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.apply_recommendations(vec![
            ModelEntry::with_source("deepseek-v4-pro", None, ModelSource::Catalog),
            ModelEntry::with_source("deepseek-v4-flash", None, ModelSource::Catalog),
        ]);
        assert_eq!(state.models().len(), 2);
        assert!(state
            .models()
            .iter()
            .all(|m| m.source == ModelSource::Catalog));
        // 再写一次：目录来源被替换，不累积
        state.apply_recommendations(vec![ModelEntry::with_source(
            "deepseek-v4-pro",
            None,
            ModelSource::Catalog,
        )]);
        assert_eq!(state.models().len(), 1);
    }

    #[test]
    fn discovery_replaces_only_its_own_entries() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.apply_recommendations(vec![ModelEntry::with_source(
            "catalog-model",
            None,
            ModelSource::Catalog,
        )]);
        state.add_model(ModelEntry::with_source(
            "manual-model",
            None,
            ModelSource::Manual,
        ));
        state.apply_discovery(vec![
            ModelEntry::with_source("live-a", None, ModelSource::Discovered),
            ModelEntry::with_source("live-b", None, ModelSource::Discovered),
        ]);
        let ids = |s: &SettingsState| -> Vec<String> {
            s.models().iter().map(|m| m.model_id.clone()).collect()
        };
        assert!(ids(&state).contains(&"live-a".to_string()));
        assert!(
            ids(&state).contains(&"catalog-model".to_string()),
            "目录推荐不该被实时结果清掉"
        );
        assert!(
            ids(&state).contains(&"manual-model".to_string()),
            "手填条目不该被清掉"
        );

        // 再拉一次：只替换实时来源的条目
        state.apply_discovery(vec![ModelEntry::with_source(
            "live-c",
            None,
            ModelSource::Discovered,
        )]);
        assert!(ids(&state).contains(&"live-c".to_string()));
        assert!(
            !ids(&state).contains(&"live-a".to_string()),
            "上一轮实时结果应被替换"
        );
        assert!(ids(&state).contains(&"catalog-model".to_string()));
    }

    #[test]
    fn every_preset_brings_out_a_usable_default() {
        for preset in crate::preset::BUILTIN_PRESETS {
            let mut state = SettingsState::new();
            state.select_provider(preset.id);
            assert_eq!(state.selected_provider_id(), preset.id);
            assert!(!state.available_protocols().is_empty());
            assert_eq!(state.endpoint(), preset.offerings[0].default_endpoint);
            assert!(state.endpoint_is_default());
        }
    }

    #[test]
    fn switching_protocol_brings_out_that_protocols_endpoint() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        assert_eq!(state.endpoint(), "https://api.deepseek.com/v1");
        state.set_protocol(ProtocolKind::AnthropicMessages);
        assert_eq!(state.endpoint(), "https://api.deepseek.com/anthropic");
    }

    #[test]
    fn manual_endpoint_is_preserved_and_resettable() {
        let mut state = SettingsState::new();
        state.select_provider("openai");
        state.set_endpoint("https://my-relay.example/v1");
        state.set_protocol(ProtocolKind::OpenAiResponses);
        assert_eq!(state.endpoint(), "https://my-relay.example/v1");
        assert!(!state.endpoint_is_default());
        state.reset_endpoint();
        assert_eq!(state.endpoint(), "https://api.openai.com/v1");
    }

    #[test]
    fn unsupported_protocol_is_ignored() {
        let mut state = SettingsState::new();
        state.select_provider("anthropic");
        state.set_protocol(ProtocolKind::OpenAiChat);
        assert_eq!(state.protocol(), ProtocolKind::AnthropicMessages);
    }

    #[test]
    fn local_providers_are_keyless_and_have_no_key_requirement() {
        let mut state = SettingsState::new();
        state.select_provider("ollama");
        assert!(state.keyless());
        state.select_model("qwen3:8b");
        // 免密服务不需要 Key 即可就绪
        assert!(state.is_ready(), "本地部署不该因缺 Key 被拦");
    }

    #[test]
    fn cloud_provider_reports_missing_key_as_hint_not_blocker() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.select_model("deepseek-chat");
        assert!(state.validate().contains(&"validation.key.empty_hint"));
        assert!(state.is_ready(), "先存配置后填 Key 应被允许");
    }

    #[test]
    fn context_evidence_prefers_user_override_and_flags_conflict() {
        // 参考产品那行：自动探测 256,000 · 覆盖 1,024,000 · 生效 1,024,000
        let evidence = resolve_context_window(Some(256_000), Some(256_000), Some(1_024_000));
        assert_eq!(evidence.effective, Some(1_024_000));
        assert_eq!(evidence.source, Some("user"));
        assert!(evidence.conflict, "覆盖了别的来源必须能看出来");
        assert_eq!(evidence.probed, Some(256_000));
    }

    #[test]
    fn context_evidence_uses_catalog_when_no_override() {
        let evidence = resolve_context_window(Some(128_000), Some(128_000), None);
        assert_eq!(evidence.effective, Some(128_000));
        assert_eq!(evidence.source, Some("catalog"));
        assert!(!evidence.conflict, "一致就不该报冲突");
    }

    #[test]
    fn context_evidence_without_catalog_falls_back_to_probe() {
        let evidence = resolve_context_window(None, Some(64_000), None);
        assert_eq!(evidence.effective, Some(64_000));
        assert_eq!(evidence.source, Some("probe"));
    }

    #[test]
    fn context_evidence_empty_when_nothing_known() {
        let evidence = resolve_context_window(None, None, None);
        assert_eq!(evidence.effective, None);
        assert!(!evidence.has_any());
    }

    #[test]
    fn state_context_evidence_combines_profile_probe_and_input() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.select_model("deepseek-chat");
        state.add_model(ModelEntry::new(
            "deepseek-chat",
            Some(profile_with(Some(64_000), vec![])),
        ));
        state.set_probed_context(Some(65_536));
        state.set_context_override_input("131072");

        let evidence = state.context_evidence();
        assert_eq!(evidence.catalog, Some(64_000));
        assert_eq!(evidence.probed, Some(65_536));
        assert_eq!(evidence.overridden, Some(131_072));
        assert_eq!(evidence.effective, Some(131_072));
        assert!(evidence.conflict);
    }

    #[test]
    fn override_input_zero_or_garbage_means_no_override() {
        let mut state = SettingsState::new();
        state.set_context_override_input("0");
        assert_eq!(state.context_override(), None);
        state.set_context_override_input("");
        assert_eq!(state.context_override(), None);
        state.set_context_override_input("abc");
        assert_eq!(state.context_override(), None);
        state.set_context_override_input(" 8192 ");
        assert_eq!(state.context_override(), Some(8_192));
    }

    #[test]
    fn supported_efforts_are_unknown_when_no_data() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.select_model("deepseek-chat");
        assert!(state.supported_efforts().is_empty());
        // 未知 → 不做本地降级
        assert!(state.resolve_effort(ReasoningEffort::High).is_none());
    }

    #[test]
    fn supported_efforts_drive_nearest_downgrade() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.select_model("deepseek-chat");
        state.add_model(ModelEntry::new(
            "deepseek-chat",
            Some(profile_with(
                Some(64_000),
                vec![ReasoningEffort::Low, ReasoningEffort::High],
            )),
        ));
        let resolved = state.resolve_effort(ReasoningEffort::Medium).unwrap();
        assert_eq!(resolved.effective, ReasoningEffort::Low);
        assert!(resolved.downgraded);
    }

    #[test]
    fn switching_provider_clears_previous_models() {
        let mut state = SettingsState::new();
        state.select_provider("openai");
        state.apply_recommendations(vec![ModelEntry::with_source(
            "gpt-4o-mini",
            None,
            ModelSource::Catalog,
        )]);
        assert!(!state.models().is_empty());
        state.select_provider("anthropic");
        // 换厂商后上家的模型必须清空（协议/端点/凭据都变了）
        assert!(state.models().is_empty(), "换厂商应清空模型列表");
        assert_eq!(state.selected_model(), None);
    }

    #[test]
    fn discovery_keeps_recommendations_and_adds_found_models() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        let before = state.models().len();
        state.apply_discovery(vec![
            ModelEntry::new(
                "deepseek-v4-flash",
                Some(profile_with(Some(1_048_576), vec![])),
            ),
            ModelEntry::new("deepseek-v4-pro", None),
        ]);
        assert!(state.models().len() > before);
        assert_eq!(state.discovered_count(), 2);
        // 发现后自动选中第一个可用模型
        assert!(state.selected_model().is_some());
    }

    #[test]
    fn removing_a_model_clears_selection() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.select_model("deepseek-chat");
        state.remove_model("deepseek-chat");
        assert_eq!(state.selected_model(), None);
        assert!(state.validate().contains(&"validation.model.required"));
    }

    #[test]
    fn search_filters_providers() {
        let mut state = SettingsState::new();
        state.search = "silicon".into();
        let found = state.filtered_presets(|p| p.id.to_string());
        assert!(found.iter().any(|p| p.id == "siliconflow"));
        assert!(found.len() < crate::preset::BUILTIN_PRESETS.len());

        // 本地化名称搜索（中文）
        state.search = "智谱".into();
        let found = state.filtered_presets(|p| {
            if p.id == "zhipu" {
                "智谱 GLM".to_string()
            } else {
                p.id.to_string()
            }
        });
        assert!(found.iter().any(|p| p.id == "zhipu"));
    }

    #[test]
    fn grouping_preserves_category_order() {
        let state = SettingsState::new();
        let groups = state.grouped(|p| p.id.to_string());
        let categories: Vec<ProviderCategory> = groups.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            categories,
            vec![
                ProviderCategory::China,
                ProviderCategory::Official,
                ProviderCategory::Gateway,
                ProviderCategory::Local,
                ProviderCategory::Custom
            ],
            "国内厂商必须排在最前"
        );
    }

    #[test]
    fn draft_carries_no_secret() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.set_api_key("sk-MUST-NOT-LEAK");
        state.select_model("deepseek-chat");
        let serialized = serde_json::to_string(&state.to_draft()).unwrap();
        assert!(!serialized.contains("MUST-NOT-LEAK"));
        assert!(!state.to_draft().values.contains_key("api_key"));
    }

    #[test]
    fn protocol_slug_matches_adapter_names() {
        assert_eq!(protocol_slug(ProtocolKind::OpenAiChat), "openai_chat");
        assert_eq!(
            protocol_slug(ProtocolKind::AnthropicMessages),
            "anthropic_messages"
        );
        assert_eq!(protocol_slug(ProtocolKind::Gemini), "gemini");
    }
}
