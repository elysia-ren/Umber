//! 上游原始条目的统一中间结构（规格：Source Adapter 的输出）。
//!
//! 每个 Source Adapter 只做一件事：**上游结构 → `RawModelRecord`**。
//! 之后所有处理（规范化 / 身份匹配 / 冲突解析 / 许可证）都只面对这一种结构，
//! 因此换数据源不需要动 Model Registry（规格 §2）。

use runtime_model::capability::CapabilityKind;
use runtime_model::evidence::EvidenceSource;
use runtime_model::model::Modality;
use serde::{Deserialize, Serialize};

/// 某个字段的原始取值 + 来源（规格 X.3：一切重要字段都能追溯来源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sourced<T> {
    pub value: T,
    pub source: EvidenceSource,
}

impl<T> Sourced<T> {
    pub fn new(value: T, source: EvidenceSource) -> Self {
        Self { value, source }
    }
}

/// 上游价格字段。全部 `Option`：**缺失即 unknown，不是 0**（规格 X.19）。
///
/// 上游单位不统一（OpenRouter 用"每 token 的美元字符串"，LiteLLM 用
/// "每 token 成本"），统一到 **每百万 token 美元**。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RawPricing {
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub cached_input_per_mtok: Option<f64>,
    pub reasoning_per_mtok: Option<f64>,
    pub image_per_unit: Option<f64>,
    pub audio_per_unit: Option<f64>,
    pub request_per_unit: Option<f64>,
    pub web_search_per_unit: Option<f64>,
}

impl RawPricing {
    pub fn is_empty(&self) -> bool {
        self.input_per_mtok.is_none()
            && self.output_per_mtok.is_none()
            && self.cached_input_per_mtok.is_none()
            && self.reasoning_per_mtok.is_none()
            && self.image_per_unit.is_none()
            && self.audio_per_unit.is_none()
            && self.request_per_unit.is_none()
            && self.web_search_per_unit.is_none()
    }
}

/// 上游能力位。`None` = 上游没说 → unknown（不猜，规格 X.16）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RawCapabilities {
    pub tool_call: Option<bool>,
    pub parallel_tool_call: Option<bool>,
    pub structured_output: Option<bool>,
    pub json_mode: Option<bool>,
    pub reasoning: Option<bool>,
    pub vision: Option<bool>,
    pub audio: Option<bool>,
    pub embedding: Option<bool>,
}

impl RawCapabilities {
    /// 遍历已声明的能力位（None 的跳过——不制造证据）。
    pub fn declared(&self) -> Vec<(CapabilityKind, bool)> {
        let mut out = Vec::new();
        let mut push = |kind: CapabilityKind, value: Option<bool>| {
            if let Some(v) = value {
                out.push((kind, v));
            }
        };
        push(CapabilityKind::ToolCall, self.tool_call);
        push(CapabilityKind::ParallelToolCall, self.parallel_tool_call);
        push(CapabilityKind::StructuredOutput, self.structured_output);
        push(CapabilityKind::JsonMode, self.json_mode);
        push(CapabilityKind::Reasoning, self.reasoning);
        push(CapabilityKind::Vision, self.vision);
        push(CapabilityKind::Audio, self.audio);
        push(CapabilityKind::Embeddings, self.embedding);
        out
    }
}

/// 上游原始条目（所有 Source Adapter 的统一输出）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawModelRecord {
    /// 数据来源（含来源名的 catalog 标识）。
    pub source: EvidenceSource,
    /// 上游原始键（如 `"openai/gpt-4o"`）——保留原样供追溯。
    pub source_key: String,
    /// 上游所属 provider（如 `"deepseek"` / `"azure_ai"`）。
    pub provider_hint: Option<String>,
    pub display_name: Option<String>,
    pub family: Option<String>,
    pub organization: Option<String>,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub input_modalities: Vec<Modality>,
    pub output_modalities: Vec<Modality>,
    pub capabilities: RawCapabilities,
    /// 上游给出的**原始档位标签**（未归一，如 `["max","high","low"]`）。
    /// 归一由 `runtime_model::effort::efforts_from_labels` 负责——
    /// 适配器不解释档位语义（规格 §2：适配器只做结构转换）。
    pub reasoning_effort_labels: Vec<String>,
    pub default_effort_label: Option<String>,
    pub reasoning_mandatory: Option<bool>,
    pub supported_parameters: Vec<String>,
    pub pricing: RawPricing,
    pub deprecated: bool,
    /// 上游给出的原始数据来源链接（审计用）。
    pub source_url: Option<String>,
    /// 上游声明的别名（目前仅官方覆盖层提供）——用于身份匹配的 alias 表（§9.1）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_aliases: Vec<String>,
    pub retrieved_at_unix: u64,
}

impl RawModelRecord {
    pub fn new(source: EvidenceSource, source_key: impl Into<String>, now_unix: u64) -> Self {
        Self {
            source,
            source_key: source_key.into(),
            provider_hint: None,
            display_name: None,
            family: None,
            organization: None,
            description: None,
            context_window: None,
            max_output_tokens: None,
            input_modalities: Vec::new(),
            output_modalities: Vec::new(),
            capabilities: RawCapabilities::default(),
            reasoning_effort_labels: Vec::new(),
            default_effort_label: None,
            reasoning_mandatory: None,
            supported_parameters: Vec::new(),
            pricing: RawPricing::default(),
            deprecated: false,
            source_url: None,
            raw_aliases: Vec::new(),
            retrieved_at_unix: now_unix,
        }
    }

    /// 上游键去掉 provider 前缀后的模型部分（`azure_ai/deepseek-v3.2` → `deepseek-v3.2`）。
    pub fn bare_model_id(&self) -> &str {
        self.source_key
            .rsplit_once('/')
            .map(|(_, model)| model)
            .unwrap_or(&self.source_key)
    }
}

/// 上游模态字符串 → Canonical 模态。无法识别返回 `None`（不猜）。
pub fn modality_from_label(label: &str) -> Option<Modality> {
    match label.trim().to_lowercase().as_str() {
        "text" => Some(Modality::Text),
        "image" | "vision" => Some(Modality::Image),
        "audio" => Some(Modality::Audio),
        "video" => Some(Modality::Video),
        _ => None,
    }
}

/// 每 token 价格（字符串或数字）→ 每百万 token 价格。
///
/// OpenRouter 给的是**每 token 的美元字符串**（`"0.0000003"`），
/// LiteLLM 给的是**每 token 的浮点**（`6.2e-07`）。两者都要乘 1e6。
pub fn per_token_to_per_mtok(per_token: f64) -> f64 {
    per_token * 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn per_token_converts_to_per_mtok() {
        // OpenRouter 实测值：prompt "0.0000003" → $0.30/M
        assert!((per_token_to_per_mtok(0.0000003) - 0.30).abs() < 1e-9);
        // LiteLLM 实测值：input_cost_per_token 6.2e-07 → $0.62/M
        assert!((per_token_to_per_mtok(6.2e-07) - 0.62).abs() < 1e-9);
    }

    #[test]
    fn absent_capability_is_not_declared() {
        let caps = RawCapabilities {
            tool_call: Some(true),
            vision: Some(false),
            ..Default::default()
        };
        let declared = caps.declared();
        assert!(declared.contains(&(CapabilityKind::ToolCall, true)));
        assert!(declared.contains(&(CapabilityKind::Vision, false)));
        // 未声明的绝不出现（unknown ≠ unsupported）
        assert!(!declared.iter().any(|(k, _)| *k == CapabilityKind::Audio));
        assert!(!declared
            .iter()
            .any(|(k, _)| *k == CapabilityKind::Reasoning));
    }

    #[test]
    fn bare_model_id_strips_provider_prefix() {
        let record = RawModelRecord::new(
            EvidenceSource::ThirdPartyCatalog {
                name: "litellm".into(),
            },
            "azure_ai/deepseek-v3.2",
            0,
        );
        assert_eq!(record.bare_model_id(), "deepseek-v3.2");
    }

    #[test]
    fn unknown_modality_returns_none() {
        assert_eq!(modality_from_label("text"), Some(Modality::Text));
        assert_eq!(modality_from_label("IMAGE"), Some(Modality::Image));
        assert_eq!(modality_from_label("hologram"), None);
    }

    #[test]
    fn record_roundtrips_through_json() {
        let mut record = RawModelRecord::new(
            EvidenceSource::ThirdPartyCatalog {
                name: "models_dev".into(),
            },
            "deepseek/deepseek-chat",
            1_700_000_000,
        );
        record.context_window = Some(64_000);
        record.reasoning_effort_labels = vec!["max".into(), "low".into()];
        record.pricing.input_per_mtok = Some(0.28);
        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["context_window"], json!(64_000));
        let back: RawModelRecord = serde_json::from_value(value).unwrap();
        assert_eq!(back, record);
    }
}
