//! OpenRouter Source Adapter（实测形状，2026-09，436 个模型）。
//!
//! ```text
//! { "data": [ {
//!     "id": "deepseek/deepseek-v4.1-flash",
//!     "canonical_slug": "...", "hugging_face_id": "...",
//!     "name": "DeepSeek: DeepSeek V4.1 Flash",
//!     "context_length": 1048576,
//!     "architecture": { "input_modalities": ["text","image"],
//!                       "output_modalities": ["text"], "tokenizer": "DeepSeek" },
//!     "pricing": { "prompt": "0.0000003", "completion": "0.0000012",
//!                  "input_cache_read": "0.000000006" },
//!     "top_provider": { "context_length": 1048576,
//!                       "max_completion_tokens": 384000, "is_moderated": false },
//!     "supported_parameters": ["tools","temperature","reasoning_effort", ...],
//!     "reasoning": { "mandatory": false, "default_enabled": true,
//!                    "supported_efforts": ["max","high","low"],
//!                    "default_effort": "high" }
//! } ] }
//! ```
//!
//! **这是唯一直接给出思考强度档位的上游**（`reasoning.supported_efforts`），
//! 因此它是"每个模型思考强度"能力的现实数据基础。档位标签原样保留，
//! 归一与就近降级交给 `runtime_model::effort`。

use runtime_model::evidence::EvidenceSource;
use serde_json::Value;

use crate::record::{
    modality_from_label, per_token_to_per_mtok, RawCapabilities, RawModelRecord, RawPricing,
};
use crate::sources::{now_unix, price_f64, SourceAdapter, SourceError};

pub struct OpenRouterAdapter {
    pub snapshot: String,
    pub retrieved_at_unix: u64,
}

impl OpenRouterAdapter {
    pub fn new(snapshot: impl Into<String>) -> Self {
        Self {
            snapshot: snapshot.into(),
            retrieved_at_unix: now_unix(),
        }
    }
}

impl Default for OpenRouterAdapter {
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

impl SourceAdapter for OpenRouterAdapter {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    fn snapshot(&self) -> String {
        self.snapshot.clone()
    }

    fn parse(&self, upstream: &Value) -> Result<Vec<RawModelRecord>, SourceError> {
        let items = upstream
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| SourceError::new(self.name(), "missing `data` array"))?;
        let source = EvidenceSource::ThirdPartyCatalog {
            name: self.name().to_string(),
        };

        let mut records = Vec::new();
        for item in items {
            let Some(id) = item.get("id").and_then(|i| i.as_str()) else {
                continue;
            };
            let mut record = RawModelRecord::new(source.clone(), id, self.retrieved_at_unix);
            record.provider_hint = id.split_once('/').map(|(p, _)| p.to_string());
            record.organization = record.provider_hint.clone();
            record.display_name = item
                .get("name")
                .and_then(|n| n.as_str())
                .map(str::to_string);
            record.description = item
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string);
            record.family = item
                .get("canonical_slug")
                .and_then(|s| s.as_str())
                .map(str::to_string)
                .or_else(|| record.provider_hint.clone());

            record.context_window = item.get("context_length").and_then(|c| c.as_u64());
            // max_completion_tokens 比 top_provider.context_length 更贴近"输出上限"
            record.max_output_tokens = item
                .get("top_provider")
                .and_then(|t| t.get("max_completion_tokens"))
                .and_then(|m| m.as_u64());

            let arch = item.get("architecture");
            record.input_modalities = modalities(arch.and_then(|a| a.get("input_modalities")));
            record.output_modalities = modalities(arch.and_then(|a| a.get("output_modalities")));

            let params: Vec<String> = item
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
            let has = |name: &str| params.iter().any(|p| p == name);
            let vision = record
                .input_modalities
                .contains(&runtime_model::model::Modality::Image);
            let audio = record
                .input_modalities
                .contains(&runtime_model::model::Modality::Audio);
            record.capabilities = RawCapabilities {
                tool_call: if has("tools") { Some(true) } else { None },
                parallel_tool_call: if has("parallel_tool_calls") {
                    Some(true)
                } else {
                    None
                },
                structured_output: if has("response_format") {
                    Some(true)
                } else {
                    None
                },
                json_mode: if has("response_format") {
                    Some(true)
                } else {
                    None
                },
                reasoning: if has("reasoning")
                    || has("include_reasoning")
                    || has("reasoning_effort")
                {
                    Some(true)
                } else {
                    None
                },
                vision: if vision { Some(true) } else { None },
                audio: if audio { Some(true) } else { None },
                embedding: None,
            };
            record.supported_parameters = params;

            // 唯一直接给档位的上游：原样保留标签，由 effort 模块归一
            if let Some(reasoning) = item.get("reasoning") {
                record.reasoning_effort_labels = reasoning
                    .get("supported_efforts")
                    .and_then(|e| e.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|i| i.as_str())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                record.default_effort_label = reasoning
                    .get("default_effort")
                    .and_then(|e| e.as_str())
                    .map(str::to_string);
                record.reasoning_mandatory = reasoning.get("mandatory").and_then(|m| m.as_bool());
            }

            // 价格是"每 token 美元"的字符串形式
            let pricing = item.get("pricing");
            record.pricing = RawPricing {
                input_per_mtok: pricing
                    .and_then(|p| p.get("prompt"))
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                output_per_mtok: pricing
                    .and_then(|p| p.get("completion"))
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                cached_input_per_mtok: pricing
                    .and_then(|p| p.get("input_cache_read"))
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                reasoning_per_mtok: pricing
                    .and_then(|p| p.get("internal_reasoning"))
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                image_per_unit: pricing.and_then(|p| p.get("image")).and_then(price_f64),
                audio_per_unit: pricing.and_then(|p| p.get("audio")).and_then(price_f64),
                request_per_unit: pricing.and_then(|p| p.get("request")).and_then(price_f64),
                web_search_per_unit: pricing
                    .and_then(|p| p.get("web_search"))
                    .and_then(price_f64),
            };

            records.push(record);
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

    fn real_shaped_entry() -> Value {
        json!({
            "id": "deepseek/deepseek-v4.1-flash",
            "canonical_slug": "deepseek/deepseek-v4.1-flash-20260910",
            "name": "DeepSeek: DeepSeek V4.1 Flash",
            "context_length": 1048576,
            "architecture": {
                "input_modalities": ["text","image"],
                "output_modalities": ["text"],
                "tokenizer": "DeepSeek"
            },
            "pricing": {
                "prompt": "0.0000003",
                "completion": "0.0000012",
                "input_cache_read": "0.000000006"
            },
            "top_provider": {
                "context_length": 1048576,
                "max_completion_tokens": 384000,
                "is_moderated": false
            },
            "supported_parameters": ["tools","temperature","reasoning_effort","response_format","parallel_tool_calls"],
            "reasoning": {
                "mandatory": false,
                "default_enabled": true,
                "supported_efforts": ["max","high","low"],
                "default_effort": "high"
            }
        })
    }

    #[test]
    fn parses_real_shape_including_reasoning_efforts() {
        let upstream = json!({"data": [real_shaped_entry()]});
        let records = OpenRouterAdapter::default().parse(&upstream).unwrap();
        let r = &records[0];
        assert_eq!(r.source_key, "deepseek/deepseek-v4.1-flash");
        assert_eq!(r.provider_hint.as_deref(), Some("deepseek"));
        assert_eq!(r.context_window, Some(1_048_576));
        assert_eq!(
            r.max_output_tokens,
            Some(384_000),
            "取 top_provider.max_completion_tokens"
        );
        // 档位标签原样保留（不归一、不猜）
        assert_eq!(r.reasoning_effort_labels, vec!["max", "high", "low"]);
        assert_eq!(r.default_effort_label.as_deref(), Some("high"));
        assert_eq!(r.reasoning_mandatory, Some(false));
        // 价格字符串 → 每百万
        let input = r.pricing.input_per_mtok.unwrap();
        assert!((input - 0.30).abs() < 1e-9, "got {input}");
        let cached = r.pricing.cached_input_per_mtok.unwrap();
        assert!((cached - 0.006).abs() < 1e-9, "got {cached}");
    }

    #[test]
    fn modalities_drive_vision_and_audio() {
        let upstream = json!({"data": [real_shaped_entry()]});
        let records = OpenRouterAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records[0].capabilities.vision, Some(true));
        assert_eq!(
            records[0].capabilities.audio, None,
            "没有 audio 模态就不声明"
        );
        assert!(records[0]
            .output_modalities
            .contains(&runtime_model::model::Modality::Text));
    }

    #[test]
    fn supported_parameters_drive_capabilities() {
        let mut entry = real_shaped_entry();
        entry["supported_parameters"] = json!(["temperature"]);
        let records = OpenRouterAdapter::default()
            .parse(&json!({"data": [entry]}))
            .unwrap();
        // 没有 tools / response_format 参数 → 不声明这两个能力（而不是声明 false）
        assert_eq!(records[0].capabilities.tool_call, None);
        assert_eq!(records[0].capabilities.structured_output, None);
        assert_eq!(records[0].capabilities.reasoning, None);
    }

    #[test]
    fn missing_reasoning_block_leaves_efforts_unknown() {
        let mut entry = real_shaped_entry();
        entry.as_object_mut().unwrap().remove("reasoning");
        let records = OpenRouterAdapter::default()
            .parse(&json!({"data": [entry]}))
            .unwrap();
        assert!(records[0].reasoning_effort_labels.is_empty());
        assert_eq!(records[0].default_effort_label, None);
    }

    #[test]
    fn shape_change_is_diagnosable() {
        let err = OpenRouterAdapter::default().parse(&json!({})).unwrap_err();
        assert!(err.reason.contains("`data` array"));
    }
}
