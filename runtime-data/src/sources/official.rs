//! 官方数据覆盖层（规格 §2 的 `official.rs`）。
//!
//! "Provider Official Data" 没有统一 API，因此以**人工核对的 JSON 覆盖层**
//! 形式提供：由维护者把厂商官方文档里的事实写进来，作为最高优先级的静态证据。
//!
//! 形状（`official.json`）：
//!
//! ```json
//! {
//!   "entries": [{
//!     "model": "deepseek-chat",
//!     "provider": "deepseek",
//!     "aliases": ["deepseek_v3"],
//!     "organization": "deepseek-ai",
//!     "context_window": 65536,
//!     "max_output_tokens": 8192,
//!     "tool_call": true,
//!     "reasoning": false,
//!     "vision": false,
//!     "input_modalities": ["text"],
//!     "output_modalities": ["text"],
//!     "supported_parameters": ["temperature","tools"],
//!     "source_url": "https://api-docs.deepseek.com/quick_start/pricing"
//!   }]
//! }
//! ```
//!
//! 与第三方目录的区别只在 `EvidenceSource::OfficialDocs`——**它不覆盖别人，
//! 它只是另一个来源**；最终谁生效由 Resolver 按字段类别裁决（规格 X.9）。

use runtime_model::evidence::EvidenceSource;
use serde_json::Value;

use crate::record::{modality_from_label, RawCapabilities, RawModelRecord, RawPricing};
use crate::sources::{now_unix, price_f64, SourceAdapter, SourceError};

pub struct OfficialOverlay {
    pub snapshot: String,
    pub retrieved_at_unix: u64,
}

impl OfficialOverlay {
    pub fn new(snapshot: impl Into<String>) -> Self {
        Self {
            snapshot: snapshot.into(),
            retrieved_at_unix: now_unix(),
        }
    }
}

impl Default for OfficialOverlay {
    fn default() -> Self {
        Self::new("curated")
    }
}

fn modalities(value: Option<&Value>) -> Vec<runtime_model::model::Modality> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str())
                .filter_map(modality_from_label)
                .collect()
        })
        .unwrap_or_default()
}

impl SourceAdapter for OfficialOverlay {
    fn name(&self) -> &'static str {
        "official"
    }

    fn snapshot(&self) -> String {
        self.snapshot.clone()
    }

    fn parse(&self, upstream: &Value) -> Result<Vec<RawModelRecord>, SourceError> {
        let entries = upstream
            .get("entries")
            .and_then(|e| e.as_array())
            .ok_or_else(|| SourceError::new(self.name(), "missing `entries` array"))?;
        let source = EvidenceSource::OfficialDocs;

        let mut records = Vec::new();
        for entry in entries {
            let Some(model) = entry.get("model").and_then(|m| m.as_str()) else {
                return Err(SourceError::new(self.name(), "entry without `model` field"));
            };
            let mut record = RawModelRecord::new(source.clone(), model, self.retrieved_at_unix);
            record.provider_hint = entry
                .get("provider")
                .and_then(|p| p.as_str())
                .map(str::to_string);
            record.organization = entry
                .get("organization")
                .and_then(|o| o.as_str())
                .map(str::to_string);
            record.display_name = entry
                .get("display_name")
                .and_then(|d| d.as_str())
                .map(str::to_string);
            record.source_url = entry
                .get("source_url")
                .and_then(|s| s.as_str())
                .map(str::to_string);

            record.context_window = entry.get("context_window").and_then(|c| c.as_u64());
            record.max_output_tokens = entry.get("max_output_tokens").and_then(|c| c.as_u64());

            record.input_modalities = modalities(entry.get("input_modalities"));
            record.output_modalities = modalities(entry.get("output_modalities"));

            let flag = |name: &str| entry.get(name).and_then(|v| v.as_bool());
            record.capabilities = RawCapabilities {
                tool_call: flag("tool_call"),
                parallel_tool_call: flag("parallel_tool_call"),
                structured_output: flag("structured_output"),
                json_mode: flag("json_mode"),
                reasoning: flag("reasoning"),
                vision: flag("vision"),
                audio: flag("audio"),
                embedding: flag("embedding"),
            };

            record.reasoning_effort_labels = entry
                .get("reasoning_efforts")
                .and_then(|e| e.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.as_str())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            record.default_effort_label = entry
                .get("default_reasoning_effort")
                .and_then(|e| e.as_str())
                .map(str::to_string);

            record.supported_parameters = entry
                .get("supported_parameters")
                .and_then(|p| p.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.as_str())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            if let Some(cost) = entry.get("cost") {
                record.pricing = RawPricing {
                    input_per_mtok: cost.get("input").and_then(price_f64),
                    output_per_mtok: cost.get("output").and_then(price_f64),
                    cached_input_per_mtok: cost.get("cache_read").and_then(price_f64),
                    ..Default::default()
                };
            }

            // aliases 挂在 record 之外——由 pipeline 通过 source_key 匹配时读取。
            // 这里把它编码进 source_key 的补充字段，保持 RawModelRecord 形状统一。
            if let Some(aliases) = entry.get("aliases").and_then(|a| a.as_array()) {
                let mut key = record.raw_aliases;
                for alias in aliases.iter().filter_map(|a| a.as_str()) {
                    key.push(alias.to_string());
                }
                record.raw_aliases = key;
            }

            records.push(record);
        }

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_curated_entry() {
        let upstream = json!({
            "entries": [{
                "model": "deepseek-chat",
                "provider": "deepseek",
                "aliases": ["deepseek_v3"],
                "organization": "deepseek-ai",
                "context_window": 65536,
                "max_output_tokens": 8192,
                "tool_call": true,
                "reasoning": false,
                "input_modalities": ["text"],
                "output_modalities": ["text"],
                "supported_parameters": ["temperature","tools"],
                "cost": {"input": 0.27, "output": 1.1, "cache_read": 0.07},
                "source_url": "https://api-docs.deepseek.com/quick_start/pricing"
            }]
        });
        let records = OfficialOverlay::default().parse(&upstream).unwrap();
        let r = &records[0];
        assert_eq!(r.source, EvidenceSource::OfficialDocs);
        assert_eq!(r.source_key, "deepseek-chat");
        assert_eq!(r.context_window, Some(65_536));
        assert_eq!(r.capabilities.tool_call, Some(true));
        // 官方说 false 是**明确的 false**（不是 unknown）——这是官方的价值
        assert_eq!(r.capabilities.reasoning, Some(false));
        assert_eq!(r.pricing.input_per_mtok, Some(0.27));
        assert_eq!(r.raw_aliases, vec!["deepseek_v3"]);
        assert!(r.source_url.is_some());
    }

    #[test]
    fn entry_without_model_is_rejected() {
        let err = OfficialOverlay::default()
            .parse(&json!({"entries": [{"provider": "x"}]}))
            .unwrap_err();
        assert!(err.reason.contains("without `model`"));
    }

    #[test]
    fn empty_overlay_is_valid_and_yields_no_records() {
        // 官方覆盖层允许为空（维护者还没填），不是错误
        let records = OfficialOverlay::default()
            .parse(&json!({"entries": []}))
            .unwrap();
        assert!(records.is_empty());
    }
}
