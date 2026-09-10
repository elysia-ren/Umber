//! 数据来源（总案 §11 §14）。

use serde::{Deserialize, Serialize};

/// 模型信息来源（总案 §11）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceSource {
    ProviderApi,
    OfficialDocs,
    BundledCatalog,
    ThirdPartyCatalog {
        name: String,
    },
    /// Runtime 探测结果。Active Probe 产生消耗，默认关闭（总案 §17）。
    Probe {
        tested_at_unix: u64,
    },
    /// 用户手动覆盖。是该 Deployment 上最终运行时值（总案 §13 §48）。
    User,
}

/// 一条证据（总案 §14）。
///
/// `confidence` 是人工设定的每来源固定先验，只用于展示与排序，
/// 不参与自动加权仲裁。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: EvidenceSource,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 每来源固定先验表（总案 §14）。仅用于展示与排序。
pub fn source_prior(source: &EvidenceSource) -> f32 {
    match source {
        EvidenceSource::OfficialDocs => 1.0,
        EvidenceSource::ProviderApi => 0.95,
        EvidenceSource::Probe { .. } => 0.9,
        EvidenceSource::BundledCatalog => 0.85,
        EvidenceSource::ThirdPartyCatalog { .. } => 0.7,
        EvidenceSource::User => 1.0,
    }
}

impl Evidence {
    pub fn new(source: EvidenceSource) -> Self {
        let verified_at_unix = match &source {
            EvidenceSource::Probe { tested_at_unix } => Some(*tested_at_unix),
            _ => None,
        };
        Self {
            confidence: source_prior(&source),
            source,
            verified_at_unix,
            note: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_evidence_carries_tested_at() {
        let e = Evidence::new(EvidenceSource::Probe { tested_at_unix: 42 });
        assert_eq!(e.verified_at_unix, Some(42));
        assert_eq!(
            e.confidence,
            source_prior(&EvidenceSource::Probe { tested_at_unix: 0 })
        );
    }
}
