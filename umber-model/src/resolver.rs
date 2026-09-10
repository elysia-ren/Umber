//! 字段级数据优先级仲裁（总案 §13）。
//!
//! 不存在全局线性优先级。不同字段的事实价值属于不同来源，
//! 按字段类别建立优先级表；用户显式覆盖是该 Deployment 上的最终运行时值。
//!
//! Resolver 算法（正式规则，简单可解释）：
//! 1. 收集所有来源候选值
//! 2. 用户显式覆盖存在 → 采用用户值，冲突记录保留
//! 3. 否则按字段类别优先级表取最高有效来源
//! 4. 次高来源与结果冲突且差值显著 → 标记 conflict
//! 5. 输出 value + winner + conflict
//!
//! `confidence` 只是来源先验，用于展示与排序，不参与自动加权仲裁。

use serde::{Deserialize, Serialize};

use crate::evidence::EvidenceSource;

/// 数字型字段的显著差异阈值：相对差超过 5% 记为冲突。
pub const NUMERIC_CONFLICT_TOLERANCE: f64 = 0.05;

/// 字段类别（总案 §13）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldCategory {
    /// tool_call / streaming / json_mode 等真实行为。
    Behavior,
    /// context_window / max_output_tokens / modalities 等规格。
    Spec,
    /// 价格。
    Pricing,
    /// canonical_id / aliases / family。
    Identity,
}

/// 来源种类（剥离 Probe 时间戳等数据后的判别）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Probe,
    ProviderApi,
    OfficialDocs,
    BundledCatalog,
    ThirdPartyCatalog,
    User,
}

fn kind_of(source: &EvidenceSource) -> SourceKind {
    match source {
        EvidenceSource::Probe { .. } => SourceKind::Probe,
        EvidenceSource::ProviderApi => SourceKind::ProviderApi,
        EvidenceSource::OfficialDocs => SourceKind::OfficialDocs,
        EvidenceSource::BundledCatalog => SourceKind::BundledCatalog,
        EvidenceSource::ThirdPartyCatalog { .. } => SourceKind::ThirdPartyCatalog,
        EvidenceSource::User => SourceKind::User,
    }
}

/// 字段类别优先级表（高 → 低，总案 §13）。
///
/// 未列入该类别的来源一律排最低（例如 Probe 对规格类只有间接证据价值）。
fn precedence(category: FieldCategory) -> &'static [SourceKind] {
    match category {
        FieldCategory::Behavior => &[
            SourceKind::Probe,
            SourceKind::ProviderApi,
            SourceKind::OfficialDocs,
            SourceKind::BundledCatalog,
            SourceKind::ThirdPartyCatalog,
        ],
        FieldCategory::Spec => &[
            SourceKind::OfficialDocs,
            SourceKind::ProviderApi,
            SourceKind::BundledCatalog,
            SourceKind::ThirdPartyCatalog,
            SourceKind::Probe,
        ],
        FieldCategory::Pricing => &[
            SourceKind::ProviderApi,
            SourceKind::OfficialDocs,
            SourceKind::BundledCatalog,
            SourceKind::ThirdPartyCatalog,
        ],
        FieldCategory::Identity => &[
            SourceKind::BundledCatalog,
            SourceKind::ProviderApi,
            SourceKind::OfficialDocs,
            SourceKind::ThirdPartyCatalog,
        ],
    }
}

fn rank(category: FieldCategory, source: &EvidenceSource) -> u8 {
    let kind = kind_of(source);
    match precedence(category).iter().position(|k| *k == kind) {
        Some(i) => i as u8,
        // User 理论上在上层处理；未列入者排最低。
        None => u8::MAX,
    }
}

/// 单字段候选值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldCandidate {
    pub value: FieldValue,
    pub source: EvidenceSource,
}

impl FieldCandidate {
    pub fn new(value: FieldValue, source: EvidenceSource) -> Self {
        Self { value, source }
    }
}

/// 字段值。不同变体之间视为冲突。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldValue {
    U64(u64),
    F64(f64),
    Bool(bool),
    Text(String),
}

impl FieldValue {
    /// 是否与另一候选等价。数字按相对差 `NUMERIC_CONFLICT_TOLERANCE` 判定；
    /// 布尔 / 文本要求严格相等；变体不同即不等价。
    pub fn equivalent(&self, other: &FieldValue) -> bool {
        match (self, other) {
            (FieldValue::U64(a), FieldValue::U64(b)) => within_tolerance(*a as f64, *b as f64),
            (FieldValue::F64(a), FieldValue::F64(b)) => within_tolerance(*a, *b),
            (FieldValue::Bool(a), FieldValue::Bool(b)) => a == b,
            (FieldValue::Text(a), FieldValue::Text(b)) => a == b,
            _ => false,
        }
    }
}

fn within_tolerance(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let magnitude = a.abs().max(b.abs());
    if magnitude == 0.0 {
        return true;
    }
    (a - b).abs() / magnitude <= NUMERIC_CONFLICT_TOLERANCE
}

/// 仲裁结果（总案 §14：最终结论 + 冲突状态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub value: FieldValue,
    pub winner: EvidenceSource,
    /// true 表示本次结果来自用户显式覆盖。
    pub user_overridden: bool,
    /// 存在与结论显著不同的其他来源候选。
    pub conflict: bool,
}

/// 对单字段执行仲裁。候选为空返回 `None`。
pub fn resolve(category: FieldCategory, candidates: &[FieldCandidate]) -> Option<Resolution> {
    if candidates.is_empty() {
        return None;
    }

    // 1. 用户显式覆盖最优先；原始来源与冲突记录保留（总案 §13 §48）。
    let mut users = candidates
        .iter()
        .filter(|c| c.source == EvidenceSource::User);
    if let Some(user_pick) = users.next() {
        let conflict = candidates
            .iter()
            .filter(|c| c.source != EvidenceSource::User)
            .any(|c| !c.value.equivalent(&user_pick.value));
        return Some(Resolution {
            value: user_pick.value.clone(),
            winner: user_pick.source.clone(),
            user_overridden: true,
            conflict,
        });
    }

    // 2. 按字段类别优先级表取最高；同级别以先验做展示性排序（不仲裁）。
    let mut sorted: Vec<&FieldCandidate> = candidates.iter().collect();
    sorted.sort_by(|a, b| {
        rank(category, &a.source)
            .cmp(&rank(category, &b.source))
            .then_with(|| b_prior(a).total_cmp(&b_prior(b)))
    });
    let winner = sorted[0];
    let conflict = sorted[1..]
        .iter()
        .any(|c| !c.value.equivalent(&winner.value));

    Some(Resolution {
        value: winner.value.clone(),
        winner: winner.source.clone(),
        user_overridden: false,
        conflict,
    })
}

fn b_prior(c: &FieldCandidate) -> f32 {
    crate::evidence::source_prior(&c.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(v: FieldValue) -> FieldCandidate {
        FieldCandidate::new(v, EvidenceSource::OfficialDocs)
    }

    fn probe(v: FieldValue, at: u64) -> FieldCandidate {
        FieldCandidate::new(v, EvidenceSource::Probe { tested_at_unix: at })
    }

    fn api(v: FieldValue) -> FieldCandidate {
        FieldCandidate::new(v, EvidenceSource::ProviderApi)
    }

    fn catalog(v: FieldValue) -> FieldCandidate {
        FieldCandidate::new(v, EvidenceSource::BundledCatalog)
    }

    fn user(v: FieldValue) -> FieldCandidate {
        FieldCandidate::new(v, EvidenceSource::User)
    }

    #[test]
    fn empty_candidates_resolve_to_none() {
        assert!(resolve(FieldCategory::Spec, &[]).is_none());
    }

    #[test]
    fn behavior_probe_beats_official_docs() {
        let r = resolve(
            FieldCategory::Behavior,
            &[
                docs(FieldValue::Bool(false)),
                probe(FieldValue::Bool(true), 1),
            ],
        )
        .unwrap();
        assert_eq!(r.value, FieldValue::Bool(true));
        assert_eq!(r.winner, EvidenceSource::Probe { tested_at_unix: 1 });
        assert!(r.conflict);
    }

    #[test]
    fn spec_official_docs_beat_probe() {
        let r = resolve(
            FieldCategory::Spec,
            &[
                probe(FieldValue::U64(200_000), 1),
                docs(FieldValue::U64(1_000_000)),
            ],
        )
        .unwrap();
        assert_eq!(r.value, FieldValue::U64(1_000_000));
        assert_eq!(r.winner, EvidenceSource::OfficialDocs);
    }

    #[test]
    fn pricing_provider_api_beats_catalog() {
        let r = resolve(
            FieldCategory::Pricing,
            &[catalog(FieldValue::F64(1.0)), api(FieldValue::F64(1.5))],
        )
        .unwrap();
        assert_eq!(r.value, FieldValue::F64(1.5));
        assert_eq!(r.winner, EvidenceSource::ProviderApi);
    }

    #[test]
    fn identity_bundled_catalog_beats_provider_api() {
        let r = resolve(
            FieldCategory::Identity,
            &[
                api(FieldValue::Text("provider/model-x".into())),
                catalog(FieldValue::Text("official/model-x".into())),
            ],
        )
        .unwrap();
        assert_eq!(r.value, FieldValue::Text("official/model-x".into()));
        assert_eq!(r.winner, EvidenceSource::BundledCatalog);
    }

    #[test]
    fn user_override_wins_and_keeps_conflict_record() {
        let r = resolve(
            FieldCategory::Spec,
            &[
                docs(FieldValue::U64(1_000_000)),
                user(FieldValue::U64(8_192)),
            ],
        )
        .unwrap();
        assert_eq!(r.value, FieldValue::U64(8_192));
        assert!(r.user_overridden);
        assert!(r.conflict, "与官方文档的冲突必须保留记录");
    }

    #[test]
    fn numeric_within_tolerance_is_not_a_conflict() {
        let r = resolve(
            FieldCategory::Spec,
            &[
                docs(FieldValue::U64(1_000_000)),
                api(FieldValue::U64(1_010_000)), // 1% < 5%
            ],
        )
        .unwrap();
        assert!(!r.conflict);
    }

    #[test]
    fn numeric_beyond_tolerance_is_a_conflict() {
        let r = resolve(
            FieldCategory::Spec,
            &[
                docs(FieldValue::U64(1_000_000)),
                api(FieldValue::U64(2_000_000)),
            ],
        )
        .unwrap();
        assert!(r.conflict);
    }

    #[test]
    fn same_source_agreement_is_not_a_conflict() {
        let r = resolve(
            FieldCategory::Behavior,
            &[
                probe(FieldValue::Bool(true), 1),
                probe(FieldValue::Bool(true), 2),
            ],
        )
        .unwrap();
        assert!(!r.conflict);
    }
}
