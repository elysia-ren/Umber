//! CompatibilityProfile（总案 §16）。
//!
//! 即使服务宣传 "OpenAI Compatible"，Runtime 也不会自动假定 100% 兼容，
//! 而是根据实际能力建立 Compatibility Profile。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 兼容特性项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityFeature {
    Responses,
    Chat,
    Reasoning,
    ToolCall,
    ToolChoiceRequired,
    StructuredOutput,
    Multimodal,
    Streaming,
    /// Reasoning.provider_payload 多轮回传（总案 §19.1）。
    ReasoningPayloadRoundTrip,
    /// 内容块级缓存断点（总案 §20）。
    CacheControlBreakpoints,
}

/// 兼容级别（总案 §16）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityLevel {
    Native,
    Emulated,
    Partial,
    Unsupported,
    Unknown,
}

/// 兼容画像。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityProfile {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<CompatibilityFeature, CompatibilityLevel>,
}

impl CompatibilityProfile {
    pub fn set(&mut self, feature: CompatibilityFeature, level: CompatibilityLevel) {
        self.features.insert(feature, level);
    }

    /// 未记录的特性返回 `Unknown`，绝不假定兼容。
    pub fn level(&self, feature: CompatibilityFeature) -> CompatibilityLevel {
        self.features
            .get(&feature)
            .copied()
            .unwrap_or(CompatibilityLevel::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrecorded_feature_is_unknown_not_compatible() {
        let p = CompatibilityProfile::default();
        assert_eq!(
            p.level(CompatibilityFeature::ToolCall),
            CompatibilityLevel::Unknown
        );
    }
}
