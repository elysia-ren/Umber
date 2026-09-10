//! ModelInfo（总案 §10）与 Pricing（§46）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use runtime_core::DeploymentId;
use runtime_core::ReasoningEffort;

use crate::capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
use crate::compatibility::CompatibilityProfile;
use crate::evidence::Evidence;
use crate::identity::ModelIdentity;

/// Runtime 对模型的标准知识描述（总案 §10）。
///
/// 任何字段都不能假设"官方写了支持，所以永远是真的"。
/// `deployment = None` 表示 Bundled Catalog 中身份级的知识条目；
/// 运行时经 Discovery 建立后绑定具体 Deployment。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// 推理信息（请求档位映射见总案 §21.1）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningInfo {
    pub capable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_efforts: Vec<ReasoningEffort>,
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

/// 价格（总案 §46）。
///
/// 契约明示：**informational 数据，随包分发必有滞后，不构成计费依据。**
/// 成本估算由宿主基于 Usage × Pricing 自行完成。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pricing {
    pub currency: String,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_at_unix: Option<u64>,
}

impl ModelInfo {
    /// 查询能力状态；未记录的能力返回 `Unknown`。
    pub fn capability_status(&self, kind: CapabilityKind) -> CapabilityStatus {
        self.capabilities
            .get(&kind)
            .map(|r| r.status)
            .unwrap_or(CapabilityStatus::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_capability_is_unknown() {
        let info = ModelInfo {
            identity: ModelIdentity {
                canonical_id: "m".into(),
                family: "m".into(),
                version: None,
                aliases: vec![],
            },
            deployment: None,
            display_name: "M".into(),
            description: String::new(),
            capabilities: BTreeMap::new(),
            limits: ModelLimits::default(),
            modalities: ModelModalities::default(),
            reasoning: ReasoningInfo::default(),
            tool_support: ToolSupport::default(),
            structured_output: StructuredOutputInfo::default(),
            pricing: None,
            compatibility: CompatibilityProfile::default(),
            evidence: vec![],
        };
        assert_eq!(
            info.capability_status(CapabilityKind::Vision),
            CapabilityStatus::Unknown
        );
    }
}
