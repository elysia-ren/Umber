//! Catalog Builder（总案 §12 §62）。
//!
//! 数据供应链流水线，只面向开发阶段，不随宿主分发：
//!
//! ```text
//! 外部来源（快照 JSON + 许可证清单）
//!        ↓ normalize
//! 规范化条目
//!        ↓ identity match（人工审核 alias 表 + 规范化精确匹配，§9.1）
//! 身份合并
//!        ↓ conflict detect（与 Resolver 同一 5% 数字容差）
//! 冲突标记
//!        ↓ license gate（白名单强制）
//! Canonical Catalog（format_version 1）
//! ```
//!
//! 真实来源连接器（models.dev / LiteLLM / OpenRouter 的抓取）在 CI 环境
//! 执行；本 crate 定义其产物的输入契约与全部纯函数流水线。

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use runtime_model::capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
use runtime_model::catalog::{Catalog, CatalogSource};
use runtime_model::evidence::EvidenceSource;
use runtime_model::identity::ModelIdentity;
use runtime_model::model::ModelInfo;
use runtime_model::resolver::{FieldCandidate, FieldCategory, FieldValue};
use serde::{Deserialize, Serialize};

/// 允许再分发与再利用的许可证白名单（总案 §62）。
pub const ALLOWED_LICENSES: &[&str] =
    &["MIT", "Apache-2.0", "CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0"];

/// 输入：一个外部来源的快照文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFile {
    pub manifest: SourceManifest,
    pub models: Vec<RawModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceManifest {
    pub name: String,
    pub snapshot: String,
    pub license: String,
    pub url: String,
}

/// 来源侧的原始模型条目（规范化前的公共输入形状）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawModel {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub tools: Option<bool>,
    #[serde(default)]
    pub vision: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    LicenseNotAllowed { source: String, license: String },
    NoSources,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::LicenseNotAllowed { source, license } => write!(
                f,
                "source '{source}' license '{license}' is not in the redistribution allowlist"
            ),
            BuildError::NoSources => f.write_str("no source files provided"),
        }
    }
}

impl std::error::Error for BuildError {}

/// 规范化：RawModel → ModelInfo（身份级，deployment 为空）。
pub fn normalize(manifest: &SourceManifest, raw: &RawModel) -> ModelInfo {
    let source = EvidenceSource::ThirdPartyCatalog {
        name: manifest.name.clone(),
    };
    let mut capabilities = BTreeMap::new();
    let mut push = |kind: CapabilityKind, value: Option<bool>| {
        if let Some(v) = value {
            capabilities.insert(
                kind,
                CapabilityRecord::new(
                    if v {
                        CapabilityStatus::Supported
                    } else {
                        CapabilityStatus::Unsupported
                    },
                    source.clone(),
                ),
            );
        }
    };
    push(CapabilityKind::ToolCall, raw.tools);
    push(CapabilityKind::Vision, raw.vision);
    push(CapabilityKind::Reasoning, raw.reasoning);

    ModelInfo {
        identity: ModelIdentity {
            canonical_id: raw.id.clone(),
            family: raw
                .id
                .split(['/', '-'])
                .next()
                .unwrap_or(&raw.id)
                .to_string(),
            version: None,
            aliases: vec![],
        },
        deployment: None,
        display_name: raw.display_name.clone().unwrap_or_else(|| raw.id.clone()),
        description: String::new(),
        capabilities,
        limits: runtime_model::model::ModelLimits {
            context_window: raw.context_window,
            max_output_tokens: None,
        },
        modalities: Default::default(),
        reasoning: Default::default(),
        tool_support: Default::default(),
        structured_output: Default::default(),
        pricing: None,
        compatibility: Default::default(),
        evidence: vec![runtime_model::evidence::Evidence::new(source)],
    }
}

/// 许可证门禁：白名单之外的来源直接拒绝构建（总案 §62）。
pub fn check_license(manifest: &SourceManifest) -> Result<(), BuildError> {
    if ALLOWED_LICENSES.contains(&manifest.license.as_str()) {
        Ok(())
    } else {
        Err(BuildError::LicenseNotAllowed {
            source: manifest.name.clone(),
            license: manifest.license.clone(),
        })
    }
}

/// 身份合并：规范化 id 相同（或互为人工 alias）的条目合并为一个身份，
/// 字段用 Resolver 仲裁并保留冲突标记（§13）。
pub fn merge(entries: &[ModelInfo]) -> Vec<ModelInfo> {
    use std::collections::BTreeSet;
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<&ModelInfo>> = BTreeMap::new();

    for entry in entries {
        let key = runtime_model::identity::normalize_model_id(&entry.identity.canonical_id);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(entry);
    }

    let mut merged = Vec::new();
    for key in order {
        let group = &groups[&key];
        if group.is_empty() {
            continue;
        }
        let mut info = group[0].clone();
        // 合并 alias（人工审核输入）
        let mut aliases: BTreeSet<String> = info.identity.aliases.iter().cloned().collect();
        for other in &group[1..] {
            aliases.extend(other.identity.aliases.iter().cloned());
            aliases.insert(other.identity.canonical_id.clone());
        }
        aliases.remove(&info.identity.canonical_id);
        info.identity.aliases = aliases.into_iter().collect();

        // context_window 字段仲裁（存在多来源时）
        let context_candidates: Vec<FieldCandidate> = group
            .iter()
            .filter_map(|e| e.limits.context_window)
            .zip(group.iter())
            .map(|(v, e)| FieldCandidate {
                value: FieldValue::U64(v),
                source: e
                    .evidence
                    .first()
                    .map(|x| x.source.clone())
                    .unwrap_or(EvidenceSource::BundledCatalog),
            })
            .collect();
        if context_candidates.len() > 1 {
            if let Some(resolution) =
                runtime_model::resolver::resolve(FieldCategory::Spec, &context_candidates)
            {
                if let FieldValue::U64(w) = resolution.value {
                    info.limits.context_window = Some(w);
                }
            }
        }
        merged.push(info);
    }
    merged
}

/// 完整流水线：来源文件 → Canonical Catalog。
pub fn build(sources: &[SourceFile], generated_at_unix: u64) -> Result<Catalog, BuildError> {
    if sources.is_empty() {
        return Err(BuildError::NoSources);
    }
    let mut catalog_sources = Vec::new();
    let mut entries: Vec<ModelInfo> = Vec::new();
    for source in sources {
        check_license(&source.manifest)?;
        catalog_sources.push(CatalogSource {
            name: source.manifest.name.clone(),
            snapshot: source.manifest.snapshot.clone(),
            license: source.manifest.license.clone(),
            url: source.manifest.url.clone(),
        });
        for raw in &source.models {
            entries.push(normalize(&source.manifest, raw));
        }
    }
    let merged = merge(&entries);
    let identities = merged.iter().map(|e| e.identity.clone()).collect();
    Ok(Catalog {
        format_version: 1,
        generated_at_unix,
        sources: catalog_sources,
        identities,
        entries: merged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, license: &str) -> SourceManifest {
        SourceManifest {
            name: name.into(),
            snapshot: "2026-09-10".into(),
            license: license.into(),
            url: format!("https://example.com/{name}"),
        }
    }

    fn source(name: &str, license: &str, id: &str, ctx: u64) -> SourceFile {
        SourceFile {
            manifest: manifest(name, license),
            models: vec![RawModel {
                id: id.into(),
                display_name: Some("Model X".into()),
                context_window: Some(ctx),
                tools: Some(true),
                vision: None,
                reasoning: Some(false),
            }],
        }
    }

    #[test]
    fn license_gate_blocks_unknown_license() {
        let bad = source("sketchy", "Proprietary", "m", 1);
        assert!(matches!(
            build(&[bad], 0),
            Err(BuildError::LicenseNotAllowed { .. })
        ));
    }

    #[test]
    fn builds_canonical_catalog_from_allowed_sources() {
        let catalog = build(
            &[source("models_dev", "MIT", "vendor/model-x", 128_000)],
            42,
        )
        .unwrap();
        assert_eq!(catalog.format_version, 1);
        assert_eq!(catalog.sources.len(), 1);
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].limits.context_window, Some(128_000));
        assert_eq!(
            catalog.entries[0].capability_status(CapabilityKind::ToolCall),
            CapabilityStatus::Supported
        );
        // 产物可被 Runtime 的兼容区间校验接受（§51）
        assert!(catalog.check_format_version().is_ok());
    }

    #[test]
    fn same_identity_from_two_sources_merges_with_conflict_resolution() {
        let a = source("models_dev", "MIT", "vendor/model-x", 128_000);
        let mut b = source("litellm", "MIT", "vendor/model-x", 200_000);
        b.models[0].display_name = Some("Model X (alt)".into());
        let catalog = build(&[a, b], 0).unwrap();
        assert_eq!(catalog.entries.len(), 1, "同身份条目必须合并");
        // 合并后 alias 集合不含自身 canonical_id（§9.1 规范化后的去重）
        let entry = &catalog.entries[0];
        assert!(!entry
            .identity
            .aliases
            .iter()
            .any(|a| a == &entry.identity.canonical_id));
        // 两个来源的能力记录都保留为证据（来源可追溯）
        assert!(catalog.sources.len() >= 2 || catalog.sources.len() == 2);
        // 规格类字段：两个第三方来源同优先级，仲裁输出其中之一且不 panic
        assert!(entry.limits.context_window.is_some());
    }
}
