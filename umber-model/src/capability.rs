//! CapabilityRecord（总案 §15）。
//!
//! 铁律：能力不是一个未经解释的 Boolean。

use serde::{Deserialize, Serialize};

use crate::evidence::{source_prior, EvidenceSource};

/// P0 能力清单（总案 §15）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Text,
    Vision,
    Audio,
    Video,
    Reasoning,
    ToolCall,
    ParallelToolCall,
    StructuredOutput,
    JsonMode,
    Streaming,
    Embeddings,
}

/// 能力状态。`unknown` 是一等状态，不是缺省的 false。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Partial,
    Unknown,
}

/// 能力记录：状态 + 来源 + 先验 + 验证时间。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityRecord {
    pub status: CapabilityStatus,
    pub source: EvidenceSource,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at_unix: Option<u64>,
}

impl CapabilityRecord {
    /// 以来源先验构造记录。
    pub fn new(status: CapabilityStatus, source: EvidenceSource) -> Self {
        let verified_at_unix = match &source {
            EvidenceSource::Probe { tested_at_unix } => Some(*tested_at_unix),
            _ => None,
        };
        Self {
            status,
            confidence: source_prior(&source),
            source,
            verified_at_unix,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_is_not_a_bare_boolean() {
        let rec = CapabilityRecord::new(
            CapabilityStatus::Supported,
            EvidenceSource::Probe { tested_at_unix: 7 },
        );
        assert_eq!(rec.status, CapabilityStatus::Supported);
        assert_eq!(rec.verified_at_unix, Some(7));
        let json = serde_json::to_value(&rec).unwrap();
        assert_eq!(json["status"], "supported");
        assert_eq!(json["source"]["type"], "probe");
    }
}
