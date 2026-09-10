//! ResolvedModel：宿主最终读到的模型视图（规格 X.25）。
//!
//! 宿主不直接读 models.dev / LiteLLM / OpenRouter / Provider `/models` /
//! Probe 缓存，只读这一个**解析完成**的视图：
//!
//! ```text
//! Catalog + Provider Metadata + Compatibility + Probe + User Override
//!                          ↓
//!                  Evidence Resolution
//!                          ↓
//!                   ResolvedModel
//! ```
//!
//! 与 `ModelProfile` 的区别：Profile 是三层结构中的知识层（可能未绑定
//! Deployment，可能字段冲突未裁决）；ResolvedModel 是**已裁决、已绑定
//! Deployment、可直接展示**的扁平视图。

use serde::{Deserialize, Serialize};

use runtime_core::request::ReasoningEffort;
use runtime_core::DeploymentId;

use crate::capability::{CapabilityKind, CapabilityStatus};
use crate::model::{ModelLimits, ModelModalities, ParameterSupport, Pricing};

/// 一条字段的证据摘要（规格 X.25 `evidence_summary`）。
///
/// 宿主展示"这个数字从哪来"；完整证据链仍在 ModelProfile.evidence 里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSummary {
    /// 字段名（如 "context_window"）。
    pub field: String,
    /// 生效值来源（如 "official" / "bundled_catalog" / "probe" / "user"）。
    pub source: String,
    /// 是否存在与生效值冲突的其他来源（规格 X.9：冲突不抹掉）。
    pub conflict: bool,
}

/// 宿主可见的最终模型视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedModel {
    /// Deployment 引用（调用时用作 `GenerateRequest.model`）。
    pub deployment: DeploymentId,
    /// 规范化模型身份。
    pub identity: String,
    pub display_name: String,
    /// 提供服务的一方。
    pub provider: String,
    /// Provider 侧的模型 ID（诊断用；宿主不需要拿它调用）。
    pub model_id: String,
    /// 能力：只有**已知**的才出现；未记录的从不出现在这里（§X.16）。
    pub capabilities: Vec<(CapabilityKind, CapabilityStatus)>,
    pub limits: ModelLimits,
    pub modalities: ModelModalities,
    /// 实际可用的思考强度档位（空 = 未知，不猜）。
    pub supported_efforts: Vec<ReasoningEffort>,
    pub default_effort: Option<ReasoningEffort>,
    pub parameters: ParameterSupport,
    pub pricing: Option<Pricing>,
    pub evidence_summary: Vec<EvidenceSummary>,
}

impl ResolvedModel {
    /// 某能力的最终判断。未记录 → `Unknown`（不等于"不支持"）。
    pub fn capability(&self, kind: CapabilityKind) -> CapabilityStatus {
        self.capabilities
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, s)| *s)
            .unwrap_or(CapabilityStatus::Unknown)
    }

    /// 请求的思考强度在该模型上的实际生效档位。
    ///
    /// 三种结果：
    /// - `Some(resolution)`：已知受支持集合，得到生效档位（可能已降级）
    /// - `None`：档位集合未知 → 调用方应原样发送，不做本地降级（§X.16）
    pub fn resolve_effort(
        &self,
        requested: ReasoningEffort,
    ) -> Option<crate::effort::EffortResolution> {
        crate::effort::nearest(requested, &self.supported_efforts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(efforts: Vec<ReasoningEffort>) -> ResolvedModel {
        ResolvedModel {
            deployment: DeploymentId::from("dep-1"),
            identity: "deepseek-v4".into(),
            display_name: "DeepSeek V4".into(),
            provider: "deepseek".into(),
            model_id: "deepseek-chat".into(),
            capabilities: vec![],
            limits: ModelLimits::default(),
            modalities: ModelModalities::default(),
            supported_efforts: efforts,
            default_effort: None,
            parameters: ParameterSupport::default(),
            pricing: None,
            evidence_summary: vec![],
        }
    }

    #[test]
    fn unrecorded_capability_is_unknown() {
        let m = model(vec![]);
        assert_eq!(
            m.capability(CapabilityKind::Vision),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn effort_downgrades_against_real_openrouter_style_set() {
        // 实测 OpenRouter 的 [max, high, low] → {Low, High}
        let m = model(vec![ReasoningEffort::Low, ReasoningEffort::High]);
        let r = m.resolve_effort(ReasoningEffort::Medium).unwrap();
        assert_eq!(r.effective, ReasoningEffort::Low);
        assert!(r.downgraded);
    }

    #[test]
    fn unknown_effort_set_means_no_local_downgrade() {
        let m = model(vec![]);
        assert!(m.resolve_effort(ReasoningEffort::High).is_none());
    }
}
