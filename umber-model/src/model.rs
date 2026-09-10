//! ModelInfo（总案 §10）与 Pricing（§46）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use umber_core::DeploymentId;
use umber_core::ReasoningEffort;

use crate::capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
use crate::compatibility::CompatibilityProfile;
use crate::evidence::Evidence;
use crate::identity::ModelIdentity;

/// Runtime 对模型的标准知识描述（总案 §10；规格 X.2 的 **Model Profile**）。
///
/// 定义一次具体 Deployment 的完整模型知识：
/// Identity 解决"它是谁"，Deployment 解决"从哪里调用"，
/// Profile 解决"通过这个 Deployment 它现在具体能做什么"。
///
/// 任何字段都不能假设"官方写了支持，所以永远是真的"。
/// `deployment = None` 表示 Bundled Catalog 中身份级的知识条目；
/// 运行时经 Discovery 建立后绑定具体 Deployment。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub identity: ModelIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<DeploymentId>,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: BTreeMap<CapabilityKind, CapabilityRecord>,
    #[serde(default)]
    pub limits: ModelLimits,
    #[serde(default)]
    pub modalities: ModelModalities,
    #[serde(default)]
    pub reasoning: ReasoningInfo,
    #[serde(default)]
    pub tool_support: ToolSupport,
    #[serde(default)]
    pub structured_output: StructuredOutputInfo,
    #[serde(default)]
    pub parameter_support: ParameterSupport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<Pricing>,
    #[serde(default)]
    pub compatibility: CompatibilityProfile,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// 规格限制。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

/// 模态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelModalities {
    #[serde(default)]
    pub input: Vec<Modality>,
    #[serde(default)]
    pub output: Vec<Modality>,
}

/// 推理信息（请求档位映射见总案 §21.1；上游来源见规格 X.7）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningInfo {
    pub capable: bool,
    /// 该 Deployment **实际支持**的档位集合（来自上游数据库或 Probe）。
    /// 空集表示"不知道支持哪些档位"——此时不得猜，请求原样发送（§X.16）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_efforts: Vec<ReasoningEffort>,
    /// 上游声明的默认档位（如 OpenRouter 的 `reasoning.default_effort`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<ReasoningEffort>,
    /// 推理是否为强制（OpenRouter 的 `reasoning.mandatory`）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mandatory: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSupport {
    pub supported: bool,
    pub parallel: bool,
    pub tool_choice: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredOutputInfo {
    pub json_mode: bool,
    pub json_schema: bool,
}

/// 参数支持（规格 X.2 的 parameter_support）。
///
/// 上游（OpenRouter `supported_parameters`、LiteLLM `supports_*`）已提供这些事实；
/// Runtime 据此判断参数能否发送，而不是盲发等 Provider 报错。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterSupport {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported: Vec<String>,
}

impl ParameterSupport {
    pub fn with_supported(names: &[String]) -> Self {
        Self {
            supported: names.to_vec(),
            unsupported: Vec::new(),
        }
    }

    /// 三态查询：`Some(true/false)` 表示已知，`None` 表示未知（不猜，§X.16）。
    pub fn supports(&self, name: &str) -> Option<bool> {
        if self.supported.iter().any(|p| p == name) {
            return Some(true);
        }
        if self.unsupported.iter().any(|p| p == name) {
            return Some(false);
        }
        None
    }
}

/// 价格（总案 §46；规格 X.19）。
///
/// 契约明示：**informational 数据，随包分发必有滞后，不构成计费依据。**
/// 成本估算由宿主基于 Usage × Pricing 自行完成。
///
/// 每个维度都是 `Option`：**不存在的维度保持 unknown，不是 0**（规格 X.19）。
/// 价格绑定 Deployment 而非 Identity——同一模型经不同网关价格不同。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pricing {
    pub currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_per_unit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_per_unit: Option<f64>,
    /// 按请求计费（部分服务商按次收费）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_per_unit: Option<f64>,
    /// 联网/搜索附加费。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search_per_unit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_at_unix: Option<u64>,
}

impl Pricing {
    /// 只带输入/输出价格的最小构造（其余维度保持 unknown）。
    pub fn per_mtok(input: f64, output: f64) -> Self {
        Self {
            currency: "USD".into(),
            input_per_mtok: Some(input),
            output_per_mtok: Some(output),
            cached_input_per_mtok: None,
            reasoning_per_mtok: None,
            image_per_unit: None,
            audio_per_unit: None,
            request_per_unit: None,
            web_search_per_unit: None,
            effective_at_unix: None,
        }
    }
}

impl ModelProfile {
    /// 查询能力状态；未记录的能力返回 `Unknown`。
    pub fn capability_status(&self, kind: CapabilityKind) -> CapabilityStatus {
        self.capabilities
            .get(&kind)
            .map(|r| r.status)
            .unwrap_or(CapabilityStatus::Unknown)
    }

    /// 是否记录了该能力（区分"未知"与"不支持"，§X.16）。
    pub fn capability_known(&self, kind: CapabilityKind) -> bool {
        self.capabilities.contains_key(&kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::ModelIdentity;

    fn empty_profile(id: &str) -> ModelProfile {
        ModelProfile {
            identity: ModelIdentity {
                canonical_id: id.into(),
                family: id.into(),
                version: None,
                organization: None,
                aliases: vec![],
            },
            deployment: None,
            display_name: id.into(),
            description: String::new(),
            capabilities: BTreeMap::new(),
            limits: ModelLimits::default(),
            modalities: ModelModalities::default(),
            reasoning: ReasoningInfo::default(),
            tool_support: ToolSupport::default(),
            structured_output: StructuredOutputInfo::default(),
            parameter_support: ParameterSupport::default(),
            pricing: None,
            compatibility: CompatibilityProfile::default(),
            evidence: vec![],
        }
    }

    #[test]
    fn missing_capability_is_unknown_not_unsupported() {
        let info = empty_profile("m");
        assert_eq!(
            info.capability_status(CapabilityKind::Vision),
            CapabilityStatus::Unknown
        );
        assert!(!info.capability_known(CapabilityKind::Vision));
    }

    #[test]
    fn pricing_dimensions_stay_unknown_rather_than_zero() {
        // 规格 X.19：不存在的维度保持 unknown，不是 0
        let pricing = Pricing::per_mtok(0.5, 1.5);
        assert_eq!(pricing.image_per_unit, None);
        assert_eq!(pricing.audio_per_unit, None);
        assert_eq!(pricing.request_per_unit, None);
        assert_eq!(pricing.web_search_per_unit, None);
        assert_eq!(pricing.reasoning_per_mtok, None);
    }

    #[test]
    fn parameter_support_is_tri_state() {
        let support = ParameterSupport::with_supported(&["tools".into(), "temperature".into()]);
        assert_eq!(support.supports("tools"), Some(true));
        assert_eq!(support.supports("logprobs"), None, "未记录 = 未知，不猜");
    }
}
