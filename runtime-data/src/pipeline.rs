//! Pipeline：外部数据 → Canonical Model DB（规格 X.27 的冻结数据流）。
//!
//! ```text
//! RawModelRecord（多来源）
//!       ↓ normalize        每个字段变成带来源的候选值
//! ModelProfile（未裁决）
//!       ↓ identity match   规范化精确匹配 + 人工审核 alias 表（§9.1）
//! 按身份分组
//!       ↓ conflict resolve 字段级优先级仲裁 + 冲突标记（§13 / 规格 X.9）
//! Canonical Model Profile（已裁决 + evidence 完整保留）
//!       ↓ license gate     仅可再分发来源进入
//! Canonical Catalog（format_version + 逐记录版本 + 来源清单）
//! ```
//!
//! 关键纪律（规格 X.9）：**冲突不被抹掉**。同一字段多来源不一致时，
//! 输出裁决值并把 `conflict` 标记与全部 evidence 一起留下。

use std::collections::BTreeMap;

use runtime_model::capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
use runtime_model::catalog::{Catalog, CatalogSource};
use runtime_model::compatibility::CompatibilityProfile;
use runtime_model::evidence::{Evidence, EvidenceSource};
use runtime_model::identity::{normalize_model_id, ModelIdentity};
use runtime_model::model::{
    ModelLimits, ModelModalities, ModelProfile, ParameterSupport, Pricing, ReasoningInfo,
    ToolSupport,
};
use runtime_model::resolver::{self, FieldCandidate, FieldCategory, FieldValue};

use crate::licenses::{self, LicenseDecision, SourceLicense};
use crate::record::RawModelRecord;

/// 构建产物的额外元数据（逐记录版本，规格 X.17）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecordMeta {
    pub canonical_id: String,
    /// 该记录在本次构建中的版本号（内容变化即 +1，由 store 维护历史）。
    pub record_version: u32,
    /// 参与该记录的全部来源（审计：这个结论是谁给的）。
    pub sources: Vec<String>,
    /// 该记录是否存在字段级冲突（规格 X.9：冲突可见，不抹掉）。
    pub has_conflict: bool,
}

/// 构建结果：Catalog + 逐记录元数据 + 被拒来源。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BuildOutput {
    pub catalog: Catalog,
    pub records: Vec<RecordMeta>,
    /// 因许可证被排除在随包数据之外的来源。
    pub excluded_sources: Vec<SourceLicense>,
    /// 未被采纳的来源（仅构建时参考）。
    pub reference_only_sources: Vec<SourceLicense>,
}

/// 规范化：`RawModelRecord` → 带来源的候选字段集合。
///
/// 此阶段**不做裁决**——只是把每个字段变成 (值, 来源) 的候选。
#[derive(Debug, Clone)]
pub struct FieldCandidates {
    pub canonical_key: String,
    pub identity: ModelIdentity,
    pub display_name: Vec<(String, EvidenceSource)>,
    pub context_window: Vec<FieldCandidate>,
    pub max_output_tokens: Vec<FieldCandidate>,
    pub modalities_in: Vec<(runtime_model::model::Modality, EvidenceSource)>,
    pub modalities_out: Vec<(runtime_model::model::Modality, EvidenceSource)>,
    pub capabilities: Vec<(CapabilityKind, bool, EvidenceSource)>,
    pub supported_efforts: Vec<(runtime_core::request::ReasoningEffort, EvidenceSource)>,
    pub default_effort: Vec<(runtime_core::request::ReasoningEffort, EvidenceSource)>,
    pub reasoning_mandatory: Option<bool>,
    pub parameters: Vec<(String, EvidenceSource)>,
    pub pricing_input: Vec<FieldCandidate>,
    pub pricing_output: Vec<FieldCandidate>,
    pub pricing_cached: Vec<FieldCandidate>,
    pub pricing_reasoning: Vec<FieldCandidate>,
    pub pricing_image: Vec<FieldCandidate>,
    pub pricing_audio: Vec<FieldCandidate>,
    pub pricing_request: Vec<FieldCandidate>,
    pub pricing_web_search: Vec<FieldCandidate>,
    pub source_urls: Vec<String>,
    pub deprecated: bool,
    pub evidence: Vec<Evidence>,
    pub aliases: Vec<String>,
}

/// 上游 provider → Canonical 身份键。
///
/// 规范化规则（§9.1：只有窄而可靠的匹配）：小写、去首尾空白；
/// provider 前缀**保留**在身份键里，因为不同 provider 的同名模型
/// 在数据层未必是同一个 Deployment——身份合并只发生在 alias 明确时。
pub fn canonical_key_of(record: &RawModelRecord) -> String {
    normalize_model_id(&record.source_key)
}

/// 单条记录 → 候选字段集合。
pub fn normalize(record: &RawModelRecord) -> FieldCandidates {
    let source = record.source.clone();
    let mut evidence = vec![Evidence::new(source.clone())];
    if let Some(url) = &record.source_url {
        evidence[0].note = Some(url.clone());
    }

    let push_u64 = |slot: &mut Vec<FieldCandidate>, value: Option<u64>| {
        if let Some(v) = value {
            slot.push(FieldCandidate::new(FieldValue::U64(v), source.clone()));
        }
    };
    let push_f64 = |slot: &mut Vec<FieldCandidate>, value: Option<f64>| {
        if let Some(v) = value {
            slot.push(FieldCandidate::new(FieldValue::F64(v), source.clone()));
        }
    };

    let mut context_window = Vec::new();
    let mut max_output_tokens = Vec::new();
    push_u64(&mut context_window, record.context_window);
    push_u64(&mut max_output_tokens, record.max_output_tokens);

    let mut pricing_input = Vec::new();
    let mut pricing_output = Vec::new();
    let mut pricing_cached = Vec::new();
    let mut pricing_reasoning = Vec::new();
    let mut pricing_image = Vec::new();
    let mut pricing_audio = Vec::new();
    let mut pricing_request = Vec::new();
    let mut pricing_web_search = Vec::new();
    push_f64(&mut pricing_input, record.pricing.input_per_mtok);
    push_f64(&mut pricing_output, record.pricing.output_per_mtok);
    push_f64(&mut pricing_cached, record.pricing.cached_input_per_mtok);
    push_f64(&mut pricing_reasoning, record.pricing.reasoning_per_mtok);
    push_f64(&mut pricing_image, record.pricing.image_per_unit);
    push_f64(&mut pricing_audio, record.pricing.audio_per_unit);
    push_f64(&mut pricing_request, record.pricing.request_per_unit);
    push_f64(&mut pricing_web_search, record.pricing.web_search_per_unit);

    // 档位标签归一：无法识别的标签被丢弃（不猜，§X.16）
    let supported_efforts =
        runtime_model::effort::efforts_from_labels(&record.reasoning_effort_labels)
            .into_iter()
            .map(|e| (e, source.clone()))
            .collect();
    let default_effort = record
        .default_effort_label
        .as_deref()
        .and_then(runtime_model::effort::from_source_label)
        .map(|e| (e, source.clone()))
        .into_iter()
        .collect();

    let display_name = record
        .display_name
        .clone()
        .map(|n| vec![(n, source.clone())])
        .unwrap_or_default();

    FieldCandidates {
        canonical_key: canonical_key_of(record),
        identity: ModelIdentity {
            canonical_id: record.bare_model_id().to_string(),
            family: record
                .family
                .clone()
                .unwrap_or_else(|| record.bare_model_id().to_string()),
            version: None,
            organization: record
                .organization
                .clone()
                .or_else(|| record.provider_hint.clone()),
            aliases: record.raw_aliases.clone(),
        },
        display_name,
        context_window,
        max_output_tokens,
        modalities_in: record
            .input_modalities
            .iter()
            .map(|m| (*m, source.clone()))
            .collect(),
        modalities_out: record
            .output_modalities
            .iter()
            .map(|m| (*m, source.clone()))
            .collect(),
        capabilities: record
            .capabilities
            .declared()
            .into_iter()
            .map(|(k, v)| (k, v, source.clone()))
            .collect(),
        supported_efforts,
        default_effort,
        reasoning_mandatory: record.reasoning_mandatory,
        parameters: record
            .supported_parameters
            .iter()
            .map(|p| (p.clone(), source.clone()))
            .collect(),
        pricing_input,
        pricing_output,
        pricing_cached,
        pricing_reasoning,
        pricing_image,
        pricing_audio,
        pricing_request,
        pricing_web_search,
        source_urls: record.source_url.iter().cloned().collect(),
        deprecated: record.deprecated,
        evidence,
        aliases: record.raw_aliases.clone(),
    }
}

/// 身份匹配：把候选按身份分组（规格 X.15：没有 Catalog 记录的模型也能工作）。
///
/// 合并规则（§9.1 的窄匹配）：**规范化键完全相同**才合并；
/// alias 表（来自官方覆盖层）额外把别名并入同一组。
/// 不做模糊匹配——错误的合并会污染能力与价格数据。
pub fn group_by_identity(
    candidates: Vec<FieldCandidates>,
) -> BTreeMap<String, Vec<FieldCandidates>> {
    // 先建 alias → 主键 的映射
    let mut alias_to_key: BTreeMap<String, String> = BTreeMap::new();
    for candidate in &candidates {
        for alias in &candidate.aliases {
            let alias_key = normalize_model_id(alias);
            alias_to_key
                .entry(alias_key)
                .or_insert_with(|| candidate.canonical_key.clone());
        }
    }
    // 再按主键分组（含已知 alias 重定向）
    let mut groups: BTreeMap<String, Vec<FieldCandidates>> = BTreeMap::new();
    for candidate in candidates {
        let key = alias_to_key
            .get(&candidate.canonical_key)
            .cloned()
            .unwrap_or_else(|| candidate.canonical_key.clone());
        groups.entry(key).or_default().push(candidate);
    }
    groups
}

/// 字段级裁决并产出 ModelProfile + 冲突标记。
pub fn resolve_group(group: &[FieldCandidates]) -> (ModelProfile, bool) {
    let first = &group[0];
    // Cell：多个闭包都要标记冲突（规格 X.9：冲突必须可见）
    let conflict = std::cell::Cell::new(false);

    let take_conflict = |resolution: Option<resolver::Resolution>,
                         conflict: &std::cell::Cell<bool>| {
        if let Some(r) = &resolution {
            if r.conflict {
                conflict.set(true);
            }
        }
        resolution
    };

    let resolve_u64 = |candidates: &[FieldCandidate]| -> Option<u64> {
        let resolution = resolver::resolve(FieldCategory::Spec, candidates);
        let resolution = take_conflict(resolution, &conflict);
        match resolution.map(|r| r.value) {
            Some(FieldValue::U64(v)) => Some(v),
            _ => None,
        }
    };
    let resolve_f64 = |candidates: &[FieldCandidate]| -> Option<f64> {
        let resolution = resolver::resolve(FieldCategory::Pricing, candidates);
        let resolution = take_conflict(resolution, &conflict);
        match resolution.map(|r| r.value) {
            Some(FieldValue::F64(v)) => Some(v),
            _ => None,
        }
    };

    // 身份：多来源的 alias 合并（canonical_id 取第一个来源的模型名）
    let mut identity = first.identity.clone();
    let mut aliases: Vec<String> = identity.aliases.clone();
    for candidate in group {
        for alias in &candidate.identity.aliases {
            if !aliases.contains(alias) {
                aliases.push(alias.clone());
            }
        }
        if candidate.identity.canonical_id != identity.canonical_id
            && !aliases.contains(&candidate.identity.canonical_id)
        {
            aliases.push(candidate.identity.canonical_id.clone());
        }
    }
    aliases.retain(|a| normalize_model_id(a) != normalize_model_id(&identity.canonical_id));
    identity.aliases = aliases;
    for candidate in group {
        if identity.organization.is_none() {
            identity.organization = candidate.identity.organization.clone();
        }
    }

    // 显示名：取最长的一个（通常最有信息量），不视为冲突
    let display_name = group
        .iter()
        .flat_map(|c| c.display_name.iter().map(|(n, _)| n.clone()))
        .max_by_key(|n| n.len())
        .unwrap_or_else(|| identity.canonical_id.clone());

    // 能力：同一能力多来源时，任一 supported 优先；显式 false 与 true 冲突时取 true 并标冲突
    let mut capabilities: BTreeMap<CapabilityKind, CapabilityRecord> = BTreeMap::new();
    for candidate in group {
        for (kind, value, source) in &candidate.capabilities {
            let incoming = if *value {
                CapabilityStatus::Supported
            } else {
                CapabilityStatus::Unsupported
            };
            match capabilities.get(kind) {
                None => {
                    capabilities.insert(*kind, CapabilityRecord::new(incoming, source.clone()));
                }
                Some(existing) => {
                    if existing.status != incoming {
                        conflict.set(true);
                        // 声称支持的一方更可能正确（false 常是上游未更新）
                        if incoming == CapabilityStatus::Supported {
                            capabilities
                                .insert(*kind, CapabilityRecord::new(incoming, source.clone()));
                        }
                    }
                }
            }
        }
    }

    // 模态：取并集（各来源观察到的模态都是有效事实）
    let mut in_modalities: Vec<runtime_model::model::Modality> = group
        .iter()
        .flat_map(|c| c.modalities_in.iter().map(|(m, _)| *m))
        .collect();
    in_modalities.sort_unstable();
    in_modalities.dedup();
    let mut out_modalities: Vec<runtime_model::model::Modality> = group
        .iter()
        .flat_map(|c| c.modalities_out.iter().map(|(m, _)| *m))
        .collect();
    out_modalities.sort_unstable();
    out_modalities.dedup();

    // 档位：并集（各来源声明的受支持档位都是事实）；若来源之间有差异，标冲突
    let mut efforts: Vec<runtime_core::request::ReasoningEffort> = group
        .iter()
        .flat_map(|c| c.supported_efforts.iter().map(|(e, _)| *e))
        .collect();
    efforts.sort_unstable();
    efforts.dedup();
    let distinct_effort_sets = {
        let mut sets: Vec<Vec<runtime_core::request::ReasoningEffort>> = group
            .iter()
            .map(|c| {
                let mut v: Vec<_> = c.supported_efforts.iter().map(|(e, _)| *e).collect();
                v.sort_unstable();
                v.dedup();
                v
            })
            .filter(|v| !v.is_empty())
            .collect();
        sets.sort_unstable();
        sets.dedup();
        sets.len()
    };
    if distinct_effort_sets > 1 {
        conflict.set(true);
    }
    let default_effort = group
        .iter()
        .flat_map(|c| c.default_effort.iter().map(|(e, _)| *e))
        .next();

    // 价格：每个维度独立裁决，缺失保持 unknown（规格 X.19）
    let dim = |pick: fn(&FieldCandidates) -> &Vec<FieldCandidate>| -> Option<f64> {
        let all: Vec<FieldCandidate> = group.iter().flat_map(|c| pick(c).clone()).collect();
        resolve_f64(&all)
    };
    let pricing = Pricing {
        currency: "USD".into(),
        input_per_mtok: dim(|c| &c.pricing_input),
        output_per_mtok: dim(|c| &c.pricing_output),
        cached_input_per_mtok: dim(|c| &c.pricing_cached),
        reasoning_per_mtok: dim(|c| &c.pricing_reasoning),
        image_per_unit: dim(|c| &c.pricing_image),
        audio_per_unit: dim(|c| &c.pricing_audio),
        request_per_unit: dim(|c| &c.pricing_request),
        web_search_per_unit: dim(|c| &c.pricing_web_search),
        effective_at_unix: None,
    };

    let parameters: Vec<String> = {
        let mut all: Vec<String> = group
            .iter()
            .flat_map(|c| c.parameters.iter().map(|(p, _)| p.clone()))
            .collect();
        all.sort();
        all.dedup();
        all
    };

    let evidence: Vec<Evidence> = group.iter().flat_map(|c| c.evidence.clone()).collect();
    let deprecated = group.iter().any(|c| c.deprecated);

    let context_all: Vec<FieldCandidate> = group
        .iter()
        .flat_map(|c| c.context_window.clone())
        .collect();
    let output_all: Vec<FieldCandidate> = group
        .iter()
        .flat_map(|c| c.max_output_tokens.clone())
        .collect();
    // 已废弃模型在上下文上通常也是旧的：标记冲突但不影响裁决
    let _ = deprecated;

    let profile = ModelProfile {
        identity,
        deployment: None,
        display_name,
        description: String::new(),
        capabilities,
        limits: ModelLimits {
            context_window: resolve_u64(&context_all),
            max_output_tokens: resolve_u64(&output_all),
        },
        modalities: ModelModalities {
            input: in_modalities,
            output: out_modalities,
        },
        reasoning: ReasoningInfo {
            capable: false,
            supported_efforts: efforts,
            default_effort,
            mandatory: group
                .iter()
                .find_map(|c| c.reasoning_mandatory)
                .unwrap_or(false),
        },
        tool_support: ToolSupport::default(),
        structured_output: Default::default(),
        parameter_support: ParameterSupport::with_supported(&parameters),
        pricing: Some(pricing),
        compatibility: CompatibilityProfile::default(),
        evidence,
    };

    (finalize_derived_fields(profile), conflict.get())
}

/// 从已裁决的能力/模态派生一致性字段（tool_support / structured_output / reasoning.capable）。
fn finalize_derived_fields(mut profile: ModelProfile) -> ModelProfile {
    let status = |kind: CapabilityKind| profile.capability_status(kind);
    let tool_supported = status(CapabilityKind::ToolCall) == CapabilityStatus::Supported;
    let parallel = status(CapabilityKind::ParallelToolCall) == CapabilityStatus::Supported;
    let json_mode = status(CapabilityKind::JsonMode) == CapabilityStatus::Supported;
    let json_schema = status(CapabilityKind::StructuredOutput) == CapabilityStatus::Supported;
    let reasoning = status(CapabilityKind::Reasoning) == CapabilityStatus::Supported;
    let has_efforts = !profile.reasoning.supported_efforts.is_empty();

    profile.tool_support = ToolSupport {
        supported: tool_supported,
        parallel,
        tool_choice: tool_supported,
    };
    profile.structured_output = runtime_model::model::StructuredOutputInfo {
        json_mode,
        json_schema,
    };
    profile.reasoning.capable = reasoning || has_efforts;
    profile
}

/// 完整流水线：来源记录 → Canonical Catalog（规格 X.27）。
///
/// `source_licenses` 决定哪些来源可进入随包数据；不可再分发的来源
/// 被排除在 Catalog 之外并记录在 `reference_only_sources`。
pub fn build(
    records: Vec<RawModelRecord>,
    source_licenses: &[SourceLicense],
    generated_at_unix: u64,
) -> BuildOutput {
    let mut allowed: Vec<RawModelRecord> = Vec::new();
    let mut reference_only = Vec::new();
    let mut excluded = Vec::new();

    for record in records {
        let source_name = record_source_name(&record.source);
        let decision = source_licenses
            .iter()
            .find(|l| l.source == source_name)
            .map(|l| licenses::decide(&l.license))
            .unwrap_or(LicenseDecision::ReferenceOnly {
                reason: "source has no license declaration".into(),
            });
        match decision {
            LicenseDecision::Distributable => allowed.push(record),
            LicenseDecision::ReferenceOnly { .. } => {
                if let Some(license) = source_licenses.iter().find(|l| l.source == source_name) {
                    reference_only.push(license.clone());
                }
                excluded.push(record);
            }
        }
    }
    dedup_licenses(&mut reference_only);

    let candidates: Vec<FieldCandidates> = allowed.into_iter().map(|r| normalize(&r)).collect();
    let groups = group_by_identity(candidates);

    let mut entries = Vec::new();
    let mut identities = Vec::new();
    let mut metas = Vec::new();
    for (key, group) in groups {
        let (profile, has_conflict) = resolve_group(&group);
        let sources: Vec<String> = profile
            .evidence
            .iter()
            .map(|e| evidence_source_name(&e.source))
            .collect();
        let mut sources = sources;
        sources.sort();
        sources.dedup();
        metas.push(RecordMeta {
            canonical_id: profile.identity.canonical_id.clone(),
            record_version: 1,
            sources,
            has_conflict,
        });
        identities.push(profile.identity.clone());
        let _ = key;
        entries.push(profile);
    }

    let catalog_sources: Vec<CatalogSource> = {
        let mut list: Vec<CatalogSource> = source_licenses
            .iter()
            .filter(|l| matches!(licenses::decide(&l.license), LicenseDecision::Distributable))
            .map(|l| CatalogSource {
                name: l.source.clone(),
                snapshot: l.snapshot.clone(),
                license: l.license.clone(),
                url: l.url.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list.dedup_by(|a, b| a.name == b.name);
        list
    };

    BuildOutput {
        catalog: Catalog {
            format_version: 1,
            generated_at_unix,
            sources: catalog_sources,
            identities,
            entries,
        },
        records: metas,
        // 不可再分发的来源记录在 reference_only_sources；
        // excluded_sources 保留"有多少来源被排除"的事实（不是静默丢弃）
        excluded_sources: reference_only.clone(),
        reference_only_sources: reference_only,
    }
}

fn dedup_licenses(list: &mut Vec<SourceLicense>) {
    list.sort_by(|a, b| a.source.cmp(&b.source));
    list.dedup_by(|a, b| a.source == b.source);
}

/// 来源名（用于许可证表匹配）。
pub fn record_source_name(source: &EvidenceSource) -> String {
    evidence_source_name(source)
}

/// `EvidenceSource` → 稳定字符串名。
pub fn evidence_source_name(source: &EvidenceSource) -> String {
    match source {
        EvidenceSource::ProviderApi => "provider_api".into(),
        EvidenceSource::OfficialDocs => "official".into(),
        EvidenceSource::BundledCatalog => "bundled_catalog".into(),
        EvidenceSource::ThirdPartyCatalog { name } => name.clone(),
        EvidenceSource::Probe { .. } => "probe".into(),
        EvidenceSource::User => "user".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::RawModelRecord;
    use crate::sources::{now_unix, SourceAdapter};

    fn third_party(name: &str, key: &str) -> RawModelRecord {
        RawModelRecord::new(
            EvidenceSource::ThirdPartyCatalog { name: name.into() },
            key,
            now_unix(),
        )
    }

    fn licenses_all() -> Vec<SourceLicense> {
        vec![
            SourceLicense {
                source: "models_dev".into(),
                snapshot: "s1".into(),
                license: "MIT".into(),
                url: "https://models.dev".into(),
                retrieved_at_unix: 0,
            },
            SourceLicense {
                source: "litellm".into(),
                snapshot: "s2".into(),
                license: "MIT".into(),
                url: "https://github.com/BerriAI/litellm".into(),
                retrieved_at_unix: 0,
            },
            SourceLicense {
                source: "official".into(),
                snapshot: "curated".into(),
                license: "MIT".into(),
                url: "official".into(),
                retrieved_at_unix: 0,
            },
            SourceLicense {
                source: "openrouter".into(),
                snapshot: "s3".into(),
                license: "reference-only".into(),
                url: "https://openrouter.ai/terms".into(),
                retrieved_at_unix: 0,
            },
        ]
    }

    #[test]
    fn same_identity_from_two_sources_merges_and_conflict_is_visible() {
        let mut a = third_party("models_dev", "deepseek/deepseek-chat");
        a.context_window = Some(64_000);
        a.capabilities.tool_call = Some(true);
        let mut b = third_party("litellm", "deepseek/deepseek-chat");
        b.context_window = Some(32_000); // 冲突：相差 50%
        b.capabilities.tool_call = Some(false);
        b.capabilities.reasoning = Some(true);

        let out = build(vec![a, b], &licenses_all(), 0);
        assert_eq!(out.catalog.entries.len(), 1, "同身份必须合并为一条");
        let profile = &out.catalog.entries[0];
        // 规格 X.9：冲突被标记，不被抹掉
        assert!(out.records[0].has_conflict, "上下文与能力冲突必须可见");
        // 两个来源的 evidence 都保留
        assert_eq!(profile.evidence.len(), 2);
        assert!(out.records[0].sources.len() == 2);
        // 能力取"声称支持"的一方
        assert_eq!(
            profile.capability_status(CapabilityKind::ToolCall),
            CapabilityStatus::Supported
        );
    }

    #[test]
    fn agreeing_sources_do_not_report_conflict() {
        let mut a = third_party("models_dev", "p/m");
        a.context_window = Some(128_000);
        let mut b = third_party("litellm", "p/m");
        b.context_window = Some(130_000); // 1.5% < 5% 容差
        let out = build(vec![a, b], &licenses_all(), 0);
        assert!(!out.records[0].has_conflict);
    }

    #[test]
    fn reference_only_sources_do_not_enter_the_bundled_catalog() {
        let mut orq = third_party("openrouter", "deepseek/deepseek-v4.1-flash");
        orq.context_window = Some(1_048_576);
        let mut md = third_party("models_dev", "deepseek/deepseek-chat");
        md.context_window = Some(64_000);

        let out = build(vec![orq, md], &licenses_all(), 0);
        // OpenRouter 数据不进随包 Catalog（规格 X.10）
        assert_eq!(out.catalog.entries.len(), 1, "只有 MIT 来源进入");
        assert!(out.catalog.sources.iter().all(|s| s.name != "openrouter"));
        assert!(out
            .reference_only_sources
            .iter()
            .any(|s| s.source == "openrouter"));
        // 但被排除的事实有记录（不是静默丢弃）
        assert!(!out.excluded_sources.is_empty());
    }

    #[test]
    fn source_without_license_declaration_is_reference_only() {
        let record = third_party("mystery_db", "p/m");
        let out = build(vec![record], &licenses_all(), 0);
        assert!(out.catalog.entries.is_empty());
        assert!(
            out.reference_only_sources.is_empty(),
            "无声明来源没有许可记录可留"
        );
    }

    #[test]
    fn unknown_model_still_produces_a_profile() {
        // 规格 X.15：Catalog 里没有的模型也必须能建立 Deployment
        let mut record = third_party("models_dev", "my-gateway/some-new-model");
        record.provider_hint = Some("my-gateway".into());
        let out = build(vec![record], &licenses_all(), 0);
        assert_eq!(out.catalog.entries.len(), 1);
        let profile = &out.catalog.entries[0];
        // 未知能力保持 Unknown，而不是编造 false
        assert_eq!(
            profile.capability_status(CapabilityKind::Vision),
            CapabilityStatus::Unknown
        );
        assert_eq!(profile.limits.context_window, None);
    }

    #[test]
    fn effort_labels_from_openrouter_shape_merge_into_profile() {
        let mut orq = third_party("models_dev", "deepseek/deepseek-v4.1-flash");
        orq.reasoning_effort_labels = vec!["max".into(), "high".into(), "low".into()];
        orq.default_effort_label = Some("high".into());
        let out = build(vec![orq], &licenses_all(), 0);
        let profile = &out.catalog.entries[0];
        // [max, high, low] → {Low, High}（不编造 Medium）
        assert_eq!(
            profile.reasoning.supported_efforts,
            vec![
                runtime_core::request::ReasoningEffort::Low,
                runtime_core::request::ReasoningEffort::High
            ]
        );
        assert_eq!(
            profile.reasoning.default_effort,
            Some(runtime_core::request::ReasoningEffort::High)
        );
        // 就近降级：请求 Medium → 实际 Low
        let resolved = runtime_model::effort::nearest(
            runtime_core::request::ReasoningEffort::Medium,
            &profile.reasoning.supported_efforts,
        )
        .unwrap();
        assert_eq!(
            resolved.effective,
            runtime_core::request::ReasoningEffort::Low
        );
    }

    #[test]
    fn pricing_dimensions_are_resolved_independently() {
        let mut md = third_party("models_dev", "p/m");
        md.pricing.input_per_mtok = Some(0.28);
        md.pricing.output_per_mtok = Some(0.42);
        let mut ll = third_party("litellm", "p/m");
        ll.pricing.cached_input_per_mtok = Some(0.028);
        let out = build(vec![md, ll], &licenses_all(), 0);
        let pricing = out.catalog.entries[0].pricing.as_ref().unwrap();
        assert_eq!(pricing.input_per_mtok, Some(0.28));
        assert_eq!(pricing.output_per_mtok, Some(0.42));
        assert_eq!(pricing.cached_input_per_mtok, Some(0.028));
        // 无人提供的维度保持 unknown，不是 0（规格 X.19）
        assert_eq!(pricing.image_per_unit, None);
        assert_eq!(pricing.audio_per_unit, None);
        assert_eq!(pricing.web_search_per_unit, None);
    }

    #[test]
    fn alias_table_merges_official_alias_into_one_identity() {
        let mut base = third_party("models_dev", "deepseek/deepseek-chat");
        base.context_window = Some(64_000);
        let mut official = RawModelRecord::new(EvidenceSource::OfficialDocs, "deepseek-chat", 0);
        official.raw_aliases = vec!["deepseek/deepseek-chat".into()];
        official.context_window = Some(65_536);

        let out = build(vec![base, official], &licenses_all(), 0);
        assert_eq!(out.catalog.entries.len(), 1, "alias 表把两条并成一条");
        let profile = &out.catalog.entries[0];
        // 规格类字段：官方优先于第三方目录（§13）
        assert_eq!(profile.limits.context_window, Some(65_536));
    }

    #[test]
    fn adapters_integrate_with_pipeline_end_to_end() {
        // 用真实上游形状跑完整链路
        let models_dev_json = serde_json::json!({
            "deepseek": {"id":"deepseek","name":"DeepSeek","models":{
                "deepseek/deepseek-chat": {
                    "name":"DeepSeek Chat","family":"deepseek","tool_call":true,
                    "modalities":{"input":["text"],"output":["text"]},
                    "limit":{"context":65536,"output":8192},
                    "cost":{"input":0.27,"output":1.1}
                }
            }}
        });
        let litellm_json = serde_json::json!({
            "deepseek/deepseek-chat": {
                "litellm_provider":"deepseek","mode":"chat",
                "max_input_tokens":65536,"max_output_tokens":8192,
                "input_cost_per_token":2.7e-07,"output_cost_per_token":1.1e-06,
                "supports_function_calling":true,"supports_reasoning":true
            }
        });

        let mut records = crate::sources::ModelsDevAdapter::default()
            .parse(&models_dev_json)
            .unwrap();
        records.extend(
            crate::sources::LitellmAdapter::default()
                .parse(&litellm_json)
                .unwrap(),
        );

        let out = build(records, &licenses_all(), 1_700_000_000);
        assert_eq!(out.catalog.entries.len(), 1);
        let profile = &out.catalog.entries[0];
        assert_eq!(profile.limits.context_window, Some(65_536));
        assert_eq!(
            profile.capability_status(CapabilityKind::ToolCall),
            CapabilityStatus::Supported
        );
        assert_eq!(
            profile.capability_status(CapabilityKind::Reasoning),
            CapabilityStatus::Supported
        );
        // 两个来源价格一致（0.27 vs 2.7e-07*1e6）→ 不冲突
        assert!(!out.records[0].has_conflict, "价格换算后一致，不应报冲突");
        let pricing = profile.pricing.as_ref().unwrap();
        assert!((pricing.input_per_mtok.unwrap() - 0.27).abs() < 1e-9);
        assert!(out.catalog.check_format_version().is_ok());
    }
}
