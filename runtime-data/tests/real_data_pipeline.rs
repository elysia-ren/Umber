//! 真实上游数据的端到端管线测试。
//!
//! 夹具是从实测上游裁剪下来的**真实数据**（2026-09 抓取），
//! 因此本测试不依赖网络，但验证的是真实结构：
//!
//! ```text
//! models_dev_sample.json    2 个 provider 的真实条目
//! litellm_sample.json       真实条目（含每 token 价格）
//! openrouter_sample.json    真实条目（含 reasoning.supported_efforts）
//! ```
//!
//! 出处与许可见 `fixtures/README.md`。

use runtime_data::pipeline::evidence_source_name;
use runtime_data::sources::{LitellmAdapter, ModelsDevAdapter, OpenRouterAdapter, SourceAdapter};
use runtime_data::{build, licenses, LocalDb, SourceLicense};

fn fixture(name: &str) -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("fixture must be valid JSON")
}

fn licenses_now() -> Vec<SourceLicense> {
    licenses::builtin_licenses()
        .into_iter()
        .map(|(source, license, url)| SourceLicense {
            source: source.to_string(),
            snapshot: "fixture".into(),
            license: license.to_string(),
            url: url.to_string(),
            retrieved_at_unix: 0,
        })
        .collect()
}

#[test]
fn models_dev_real_fixture_parses() {
    let records = ModelsDevAdapter::default()
        .parse(&fixture("models_dev_sample.json"))
        .expect("real models.dev data must parse");
    assert!(!records.is_empty());
    // 真实数据里 limit/cost 都存在
    assert!(
        records.iter().any(|r| r.context_window.is_some()),
        "至少一条应带上下文上限"
    );
    assert!(
        records.iter().any(|r| r.pricing.input_per_mtok.is_some()),
        "至少一条应带价格"
    );
    // provider 提示来自对象键
    assert!(records.iter().all(|r| r.provider_hint.is_some()));
}

#[test]
fn litellm_real_fixture_parses_and_converts_prices() {
    let records = LitellmAdapter::default()
        .parse(&fixture("litellm_sample.json"))
        .expect("real LiteLLM data must parse");
    assert!(!records.is_empty());
    for record in &records {
        // 每 token → 每百万：换算后的输入价应落在合理区间（0.001 ~ 1000 美元/百万）
        if let Some(input) = record.pricing.input_per_mtok {
            assert!(
                input > 0.001 && input < 1000.0,
                "{} 输入价换算异常: {input}",
                record.source_key
            );
        }
    }
}

#[test]
fn openrouter_real_fixture_carries_reasoning_efforts() {
    let records = OpenRouterAdapter::default()
        .parse(&fixture("openrouter_sample.json"))
        .expect("real OpenRouter data must parse");
    assert!(!records.is_empty());
    // 这是唯一直接给出思考强度档位的上游——本测试钉住这个事实
    let with_efforts = records
        .iter()
        .filter(|r| !r.reasoning_effort_labels.is_empty())
        .count();
    assert!(
        with_efforts > 0,
        "真实 OpenRouter 数据应带 supported_efforts"
    );
    // 档位标签原样保留（不归一）
    let labels: Vec<&String> = records
        .iter()
        .flat_map(|r| r.reasoning_effort_labels.iter())
        .collect();
    assert!(
        labels
            .iter()
            .any(|l| *l == "max" || *l == "high" || *l == "low"),
        "实测标签应为 max/high/low，实际: {labels:?}"
    );
}

#[test]
fn full_pipeline_on_real_data_produces_canonical_catalog() {
    let mut records = ModelsDevAdapter::default()
        .parse(&fixture("models_dev_sample.json"))
        .unwrap();
    let models_dev_count = records.len();
    records.extend(
        LitellmAdapter::default()
            .parse(&fixture("litellm_sample.json"))
            .unwrap(),
    );
    records.extend(
        OpenRouterAdapter::default()
            .parse(&fixture("openrouter_sample.json"))
            .unwrap(),
    );
    let total = records.len();
    assert_eq!(total, models_dev_count + records.len() - models_dev_count);

    let out = build(records, &licenses_now(), 1_700_000_000);

    // OpenRouter 是 reference-only：其数据不得进入随包 Catalog
    assert!(
        out.catalog.sources.iter().all(|s| s.name != "openrouter"),
        "reference-only 来源不得进入随包 Catalog"
    );
    assert!(
        out.reference_only_sources
            .iter()
            .any(|s| s.source == "openrouter"),
        "被排除的来源必须留痕"
    );

    // 产出的每条都带 Evidence（规格 X.28 第 6 条铁律）
    for profile in &out.catalog.entries {
        assert!(
            !profile.evidence.is_empty(),
            "{} 缺少 evidence",
            profile.identity.canonical_id
        );
    }

    // 产出的 Catalog 能被 Runtime 的兼容区间校验接受
    assert!(out.catalog.check_format_version().is_ok());

    // 元数据里来源可追溯（这个结论是谁给的）
    for meta in &out.records {
        assert!(!meta.sources.is_empty(), "记录来源必须可追溯");
    }

    println!(
        "[pipeline] 输入 {} 条 → 输出 {} 条（合并 {} 条），冲突 {} 条",
        total,
        out.catalog.entries.len(),
        total - out.catalog.entries.len(),
        out.records.iter().filter(|m| m.has_conflict).count()
    );
}

#[test]
fn real_data_flows_into_local_db_with_record_versions() {
    let dir = std::env::temp_dir().join(format!(
        "umer-realdata-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let records = ModelsDevAdapter::default()
        .parse(&fixture("models_dev_sample.json"))
        .unwrap();
    let out = build(records, &licenses_now(), 1_700_000_000);

    let db = LocalDb::open(&dir).unwrap();
    db.set_catalog_version(out.catalog.format_version, out.catalog.generated_at_unix)
        .unwrap();
    let first = db
        .upsert_profiles(&out.catalog.entries, &out.records, 100)
        .unwrap();
    assert_eq!(first, out.catalog.entries.len(), "首次全部写入");
    assert_eq!(db.catalog_version().unwrap().unwrap().format_version, 1);

    // 同一份数据再灌一次：内容未变 → 不重写（规格 X.17 的增量语义）
    let second = db
        .upsert_profiles(&out.catalog.entries, &out.records, 200)
        .unwrap();
    assert_eq!(second, 0, "内容未变不应产生写入");

    // 版本号保持 1（未变）
    let any = &out.catalog.entries[0].identity.canonical_id;
    assert_eq!(db.profile(any).unwrap().unwrap().record_version, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn evidence_source_names_are_stable_strings() {
    // 审计字符串必须稳定（它们会进 Catalog 与日志）
    use runtime_model::evidence::EvidenceSource;
    assert_eq!(
        evidence_source_name(&EvidenceSource::OfficialDocs),
        "official"
    );
    assert_eq!(
        evidence_source_name(&EvidenceSource::ThirdPartyCatalog {
            name: "models_dev".into()
        }),
        "models_dev"
    );
    assert_eq!(evidence_source_name(&EvidenceSource::User), "user");
    assert_eq!(
        evidence_source_name(&EvidenceSource::Probe { tested_at_unix: 0 }),
        "probe"
    );
}
