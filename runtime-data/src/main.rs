//! `model-data` CLI：驱动模型数据管线。
//!
//! ```text
//! model-data build  <source.json>... [-o catalog.json]   # 解析 → 合并 → 出 Canonical Catalog
//! model-data inspect <catalog.json>                       # 查看产出的记录与冲突
//! model-data licenses                                     # 打印内置来源许可声明
//! ```
//!
//! 输入文件按来源自动识别（`fetch` 由 CI 负责下载快照，见 `scripts/`）。

use std::collections::BTreeMap;
use std::process::ExitCode;

use runtime_data::licenses::{self, SourceLicense};
use runtime_data::sources::{
    LitellmAdapter, ModelsDevAdapter, OfficialOverlay, OpenRouterAdapter, SourceAdapter,
};
use runtime_data::{build, LocalDb, RawModelRecord};

/// 来源识别：按上游 JSON 的形状判断该用哪个适配器。
fn detect_and_parse(text: &str, hint: Option<&str>) -> Result<Vec<RawModelRecord>, String> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;

    // 显式提示优先（文件名里的来源名）
    if let Some(hint) = hint {
        let adapter: Box<dyn SourceAdapter> = match hint {
            "models_dev" => Box::new(ModelsDevAdapter::default()),
            "litellm" => Box::new(LitellmAdapter::default()),
            "openrouter" => Box::new(OpenRouterAdapter::default()),
            "official" => Box::new(OfficialOverlay::default()),
            other => return Err(format!("unknown source hint `{other}`")),
        };
        return adapter.parse(&json).map_err(|e| e.to_string());
    }

    // 无提示：按形状探测
    let candidates: Vec<Box<dyn SourceAdapter>> = vec![
        Box::new(OfficialOverlay::default()),
        Box::new(OpenRouterAdapter::default()),
        Box::new(LitellmAdapter::default()),
        Box::new(ModelsDevAdapter::default()),
    ];
    for adapter in candidates {
        if let Ok(records) = adapter.parse(&json) {
            if !records.is_empty() {
                return Ok(records);
            }
        }
    }
    Err("could not detect source shape; pass a hint via the filename (e.g. litellm.json)".into())
}

/// 从文件名推断来源提示。
fn hint_from_path(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    for name in ["models_dev", "litellm", "openrouter", "official"] {
        if lower.contains(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// 内置来源许可声明 → SourceLicense 清单（构建期核对结果的代码化）。
fn source_licenses(now: u64) -> Vec<SourceLicense> {
    licenses::builtin_licenses()
        .into_iter()
        .map(|(source, license, url)| SourceLicense {
            source: source.to_string(),
            snapshot: "cli".into(),
            license: license.to_string(),
            url: url.to_string(),
            retrieved_at_unix: now,
        })
        .collect()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!(
            "usage:\n  model-data build <source.json>... [-o catalog.json]\n  \
             model-data inspect <catalog.json>\n  model-data licenses"
        );
        return ExitCode::FAILURE;
    };

    match command {
        "licenses" => {
            for (source, license, url) in licenses::builtin_licenses() {
                let verdict = match licenses::decide(license) {
                    licenses::LicenseDecision::Distributable => "可随包分发",
                    licenses::LicenseDecision::ReferenceOnly { .. } => "仅构建参考",
                };
                println!("{source:<12} {license:<16} {verdict:<10} {url}");
            }
            ExitCode::SUCCESS
        }
        "build" => {
            let mut inputs = Vec::new();
            let mut output = "catalog.json".to_string();
            let mut rest = args.iter().skip(1);
            while let Some(arg) = rest.next() {
                if arg == "-o" {
                    match rest.next() {
                        Some(o) => output = o.clone(),
                        None => {
                            eprintln!("error: -o requires a path");
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    inputs.push(arg.clone());
                }
            }
            if inputs.is_empty() {
                eprintln!("error: build requires at least one source file");
                return ExitCode::FAILURE;
            }

            let mut all = Vec::new();
            for path in &inputs {
                let text = match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("error: cannot read {path}: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                match detect_and_parse(&text, hint_from_path(path).as_deref()) {
                    Ok(records) => {
                        println!("[build] {path}: {} 条记录", records.len());
                        all.extend(records);
                    }
                    Err(e) => {
                        eprintln!("error: {path}: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let out = build(all, &source_licenses(now_unix()), now_unix());
            let json =
                serde_json::to_string_pretty(&out).expect("BuildOutput serialization cannot fail");
            if let Err(e) = std::fs::write(&output, json) {
                eprintln!("error: cannot write {output}: {e}");
                return ExitCode::FAILURE;
            }

            let conflicts = out.records.iter().filter(|r| r.has_conflict).count();
            println!(
                "[build] wrote {output}\n\
                 [build] models={} sources={} conflicts={} reference_only={}",
                out.catalog.entries.len(),
                out.catalog.sources.len(),
                conflicts,
                out.reference_only_sources.len()
            );
            if !out.reference_only_sources.is_empty() {
                println!("[build] 仅构建参考（不进随包数据）：");
                for s in &out.reference_only_sources {
                    println!("         - {} ({})", s.source, s.license);
                }
            }
            ExitCode::SUCCESS
        }
        "inspect" => {
            let Some(path) = args.get(1) else {
                eprintln!("error: inspect requires a catalog path");
                return ExitCode::FAILURE;
            };
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: cannot read {path}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let out: runtime_data::BuildOutput = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: not a build output: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let meta: BTreeMap<&str, &runtime_data::RecordMeta> = out
                .records
                .iter()
                .map(|m| (m.canonical_id.as_str(), m))
                .collect();
            println!(
                "models={} sources={}",
                out.catalog.entries.len(),
                out.catalog.sources.len()
            );
            for entry in &out.catalog.entries {
                let m = meta.get(entry.identity.canonical_id.as_str());
                let conflict = m.map(|m| m.has_conflict).unwrap_or(false);
                let ctx = entry
                    .limits
                    .context_window
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into());
                let efforts: Vec<&str> = entry
                    .reasoning
                    .supported_efforts
                    .iter()
                    .map(|e| e.canonical_label())
                    .collect();
                println!(
                    "{:<44} ctx={:<9} efforts={:<20} evidence={} {}",
                    entry.identity.canonical_id,
                    ctx,
                    if efforts.is_empty() {
                        "unknown".into()
                    } else {
                        efforts.join(",")
                    },
                    entry.evidence.len(),
                    if conflict { "⚠conflict" } else { "" }
                );
            }
            ExitCode::SUCCESS
        }
        "store" => {
            // 演示 Local DB：把构建产物灌入本地库，展示逐记录版本与增量语义
            let Some(path) = args.get(1) else {
                eprintln!("error: store requires a catalog path");
                return ExitCode::FAILURE;
            };
            let Some(dir) = args.get(2) else {
                eprintln!("error: store requires a target directory");
                return ExitCode::FAILURE;
            };
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: cannot read {path}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let out: runtime_data::BuildOutput = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: not a build output: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let db = match LocalDb::open(dir) {
                Ok(db) => db,
                Err(e) => {
                    eprintln!("error: cannot open local db: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let now = now_unix();
            let changed = match db.upsert_profiles(&out.catalog.entries, &out.records, now) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let _ = db.set_record_meta(&out.records);
            let _ =
                db.set_catalog_version(out.catalog.format_version, out.catalog.generated_at_unix);
            println!("[store] {dir}: 写入 {changed} 条变化记录（未变化的不重写，逐记录版本自增）");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command `{other}`");
            ExitCode::FAILURE
        }
    }
}
