//! models.dev Source Adapter（实测形状，2026-09，213 个 provider）。
//!
//! ```text
//! {
//!   "<provider_id>": {
//!     "id": "subconscious", "name": "Subconscious", "env": [...], "doc": "...",
//!     "models": {
//!       "subconscious/glm-5.2": {
//!         "id", "name", "description", "family",
//!         "attachment", "reasoning", "reasoning_options": [{"type":"toggle"}],
//!         "tool_call", "structured_output", "temperature",
//!         "release_date", "last_updated",
//!         "modalities": {"input":["text"],"output":["text"]},
//!         "open_weights": true,
//!         "limit": {"context":1000000,"output":131072},
//!         "cost": {"input":1.4,"output":4.4,"cache_read":0.26}
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! 注意：models.dev 的 `cost` 单位是**每百万 token 美元**（与 LiteLLM /
//! OpenRouter 的每 token 不同），因此这里不做 1e6 换算。

use runtime_model::evidence::EvidenceSource;
use serde_json::Value;

use crate::record::{modality_from_label, RawCapabilities, RawModelRecord, RawPricing};
use crate::sources::{now_unix, price_f64, SourceAdapter, SourceError};

pub struct ModelsDevAdapter {
    pub snapshot: String,
    pub retrieved_at_unix: u64,
}

impl ModelsDevAdapter {
    pub fn new(snapshot: impl Into<String>) -> Self {
        Self {
            snapshot: snapshot.into(),
            retrieved_at_unix: now_unix(),
        }
    }
}

impl Default for ModelsDevAdapter {
    fn default() -> Self {
        Self::new("latest")
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

impl SourceAdapter for ModelsDevAdapter {
    fn name(&self) -> &'static str {
        "models_dev"
    }

    fn snapshot(&self) -> String {
        self.snapshot.clone()
    }

    fn parse(&self, upstream: &Value) -> Result<Vec<RawModelRecord>, SourceError> {
        let providers = upstream.as_object().ok_or_else(|| {
            SourceError::new(self.name(), "top level is not an object of providers")
        })?;
        let source = EvidenceSource::ThirdPartyCatalog {
            name: self.name().to_string(),
        };

        let mut records = Vec::new();
        for (provider_id, provider) in providers {
            let Some(models) = provider.get("models").and_then(|m| m.as_object()) else {
                continue; // provider 无 models 字段：跳过而不是报错
            };
            let provider_doc = provider
                .get("doc")
                .and_then(|d| d.as_str())
                .map(str::to_string);

            for (model_key, model) in models {
                let mut record =
                    RawModelRecord::new(source.clone(), model_key, self.retrieved_at_unix);
                // provider_id 来自对象键；models.dev 的 models 键已带 provider 前缀
                record.provider_hint = Some(provider_id.clone());
                record.display_name = model
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string);
                record.family = model
                    .get("family")
                    .and_then(|f| f.as_str())
                    .map(str::to_string);
                record.organization = Some(provider_id.clone());
                record.description = model
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(str::to_string);
                record.source_url = provider_doc.clone();

                record.context_window = model
                    .get("limit")
                    .and_then(|l| l.get("context"))
                    .and_then(|c| c.as_u64());
                record.max_output_tokens = model
                    .get("limit")
                    .and_then(|l| l.get("output"))
                    .and_then(|o| o.as_u64());

                record.input_modalities =
                    modalities(model.get("modalities").and_then(|m| m.get("input")));
                record.output_modalities =
                    modalities(model.get("modalities").and_then(|m| m.get("output")));

                let vision = record
                    .input_modalities
                    .contains(&runtime_model::model::Modality::Image);
                record.capabilities = RawCapabilities {
                    tool_call: model.get("tool_call").and_then(|v| v.as_bool()),
                    structured_output: model.get("structured_output").and_then(|v| v.as_bool()),
                    reasoning: model.get("reasoning").and_then(|v| v.as_bool()),
                    // 模态里出现 image 即为 vision 能力的证据
                    vision: if vision { Some(true) } else { None },
                    ..Default::default()
                };

                // 上游只给机制不给档位词：原样保留机制名，**不据此推断档位**（§X.16）
                record.reasoning_effort_labels = model
                    .get("reasoning_options")
                    .and_then(|o| o.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|i| i.get("type").and_then(|t| t.as_str()))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();

                // cost 单位已是每百万 token（实测），不做换算
                let cost = model.get("cost");
                record.pricing = RawPricing {
                    input_per_mtok: cost.and_then(|c| c.get("input")).and_then(price_f64),
                    output_per_mtok: cost.and_then(|c| c.get("output")).and_then(price_f64),
                    cached_input_per_mtok: cost
                        .and_then(|c| c.get("cache_read"))
                        .and_then(price_f64),
                    reasoning_per_mtok: cost.and_then(|c| c.get("reasoning")).and_then(price_f64),
                    ..Default::default()
                };

                records.push(record);
            }
        }

        if records.is_empty() {
            return Err(SourceError::new(
                self.name(),
                "no models parsed; upstream shape may have changed",
            ));
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_real_shape_with_limits_and_cost() {
        let upstream = json!({
            "subconscious": {
                "id": "subconscious", "name": "Subconscious",
                "doc": "https://example.com/docs",
                "models": {
                    "subconscious/glm-5.2": {
                        "id": "subconscious/glm-5.2",
                        "name": "GLM-5.2",
                        "family": "glm",
                        "reasoning": true,
                        "reasoning_options": [{"type":"toggle"},{"type":"budget_tokens"}],
                        "tool_call": true,
                        "structured_output": true,
                        "modalities": {"input":["text"],"output":["text"]},
                        "limit": {"context": 1000000, "output": 131072},
                        "cost": {"input": 1.4, "output": 4.4, "cache_read": 0.26}
                    }
                }
            }
        });
        let records = ModelsDevAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.source_key, "subconscious/glm-5.2");
        assert_eq!(r.provider_hint.as_deref(), Some("subconscious"));
        assert_eq!(r.context_window, Some(1_000_000));
        assert_eq!(r.max_output_tokens, Some(131_072));
        // cost 已是每百万单位，不得再乘 1e6
        assert_eq!(r.pricing.input_per_mtok, Some(1.4));
        assert_eq!(r.pricing.cached_input_per_mtok, Some(0.26));
        assert_eq!(r.capabilities.tool_call, Some(true));
        assert_eq!(r.capabilities.reasoning, Some(true));
        // 机制名原样保留，不推断档位
        assert_eq!(r.reasoning_effort_labels, vec!["toggle", "budget_tokens"]);
        assert_eq!(r.source_url.as_deref(), Some("https://example.com/docs"));
    }

    #[test]
    fn vision_derived_from_input_modalities_only_when_present() {
        let upstream = json!({
            "p": {"models": {
                "p/vision-model": {"modalities": {"input": ["text","image"], "output": ["text"]}},
                "p/text-model": {"modalities": {"input": ["text"], "output": ["text"]}}
            }}
        });
        let records = ModelsDevAdapter::default().parse(&upstream).unwrap();
        let vision = records
            .iter()
            .find(|r| r.source_key == "p/vision-model")
            .unwrap();
        let text = records
            .iter()
            .find(|r| r.source_key == "p/text-model")
            .unwrap();
        assert_eq!(vision.capabilities.vision, Some(true));
        assert_eq!(
            text.capabilities.vision, None,
            "没有 image 模态就不声明 vision"
        );
        assert!(vision
            .input_modalities
            .contains(&runtime_model::model::Modality::Image));
    }

    #[test]
    fn provider_without_models_is_skipped_not_fatal() {
        let upstream = json!({
            "a": {"models": {"a/m1": {"name": "M1"}}},
            "b": {"id": "b"}
        });
        let records = ModelsDevAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn shape_change_is_a_diagnosable_error_not_panic() {
        let err = ModelsDevAdapter::default().parse(&json!([])).unwrap_err();
        assert_eq!(err.source, "models_dev");
        assert!(err.reason.contains("not an object"));

        let empty = ModelsDevAdapter::default().parse(&json!({})).unwrap_err();
        assert!(empty.reason.contains("no models parsed"));
    }
}
