//! LiteLLM Source Adapter（实测形状，2026-09，3853 条）。
//!
//! ```text
//! {
//!   "azure_ai/deepseek-v3.2": {
//!     "litellm_provider": "azure_ai",
//!     "mode": "chat",
//!     "max_input_tokens": 163840,
//!     "max_output_tokens": 163840,
//!     "max_tokens": 163840,
//!     "input_cost_per_token": 6.2e-07,
//!     "output_cost_per_token": 1.85e-06,
//!     "cache_read_input_token_cost": 3.1e-07,
//!     "supports_function_calling": true,
//!     "supports_reasoning": true,
//!     "supports_tool_choice": true,
//!     "supports_prompt_caching": true,
//!     "source": "https://...",
//!     "deprecation_date": "2027-07-01"
//!   }
//! }
//! ```
//!
//! **实测坑**：该文件存在**仅大小写不同的重复键**
//! （`together_ai/baai/bge-base-en-v1.5` vs `together_ai/BAAI/bge-base-en-v1.5`）。
//! 大小写不敏感的解析器会直接失败，因此必须用大小写敏感的 map
//! （serde_json 的 Map 默认大小写敏感，这里显式声明此事实以免后人踩）。

use runtime_model::evidence::EvidenceSource;
use serde_json::Value;

use crate::record::{per_token_to_per_mtok, RawCapabilities, RawModelRecord, RawPricing};
use crate::sources::{now_unix, price_f64, SourceAdapter, SourceError};

pub struct LitellmAdapter {
    pub snapshot: String,
    pub retrieved_at_unix: u64,
}

impl LitellmAdapter {
    pub fn new(snapshot: impl Into<String>) -> Self {
        Self {
            snapshot: snapshot.into(),
            retrieved_at_unix: now_unix(),
        }
    }

    /// 常见非对话条目（embedding / image / audio / rerank）也会出现在该文件里，
    /// 按 `mode` 过滤；`mode` 缺失时保留（宁可多不可漏，由 pipeline 去重）。
    fn is_chat_mode(mode: Option<&str>) -> bool {
        match mode {
            None => true,
            Some("chat") | Some("completion") | Some("responses") => true,
            Some(_) => false,
        }
    }
}

impl Default for LitellmAdapter {
    fn default() -> Self {
        Self::new("main")
    }
}

impl SourceAdapter for LitellmAdapter {
    fn name(&self) -> &'static str {
        "litellm"
    }

    fn snapshot(&self) -> String {
        self.snapshot.clone()
    }

    fn parse(&self, upstream: &Value) -> Result<Vec<RawModelRecord>, SourceError> {
        let entries = upstream
            .as_object()
            .ok_or_else(|| SourceError::new(self.name(), "top level is not an object"))?;
        let source = EvidenceSource::ThirdPartyCatalog {
            name: self.name().to_string(),
        };

        let mut records = Vec::new();
        for (key, entry) in entries {
            let mode = entry.get("mode").and_then(|m| m.as_str());
            if !Self::is_chat_mode(mode) {
                continue;
            }
            let mut record = RawModelRecord::new(source.clone(), key, self.retrieved_at_unix);
            record.provider_hint = entry
                .get("litellm_provider")
                .and_then(|p| p.as_str())
                .map(str::to_string)
                .or_else(|| key.split_once('/').map(|(p, _)| p.to_string()));
            record.organization = record.provider_hint.clone();
            record.source_url = entry
                .get("source")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            record.deprecated = entry
                .get("deprecation_date")
                .map(|d| !d.is_null())
                .unwrap_or(false);

            // 优先级：max_input_tokens（真输入上限）> max_tokens（部分条目只有它）
            record.context_window = entry
                .get("max_input_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| entry.get("max_tokens").and_then(|v| v.as_u64()));
            record.max_output_tokens = entry
                .get("max_output_tokens")
                .and_then(|v| v.as_u64())
                .or_else(|| entry.get("max_tokens").and_then(|v| v.as_u64()));

            let flag = |name: &str| entry.get(name).and_then(|v| v.as_bool());
            record.capabilities = RawCapabilities {
                tool_call: flag("supports_function_calling"),
                parallel_tool_call: flag("supports_parallel_function_calling"),
                structured_output: flag("supports_response_schema"),
                json_mode: flag("supports_json_mode"),
                reasoning: flag("supports_reasoning"),
                vision: flag("supports_vision"),
                audio: flag("supports_audio_input"),
                embedding: flag("supports_embedding_image_input"),
            };

            // LiteLLM 的价格单位是"每 token 美元"，必须换算到每百万
            record.pricing = RawPricing {
                input_per_mtok: entry
                    .get("input_cost_per_token")
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                output_per_mtok: entry
                    .get("output_cost_per_token")
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                cached_input_per_mtok: entry
                    .get("cache_read_input_token_cost")
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                reasoning_per_mtok: entry
                    .get("output_cost_per_reasoning_token")
                    .and_then(price_f64)
                    .map(per_token_to_per_mtok),
                image_per_unit: entry.get("input_cost_per_image").and_then(price_f64),
                audio_per_unit: entry
                    .get("input_cost_per_audio_per_second")
                    .and_then(price_f64),
                request_per_unit: entry.get("input_cost_per_request").and_then(price_f64),
                web_search_per_unit: entry
                    .get("input_cost_per_web_search_request")
                    .and_then(price_f64),
            };

            records.push(record);
        }

        if records.is_empty() {
            return Err(SourceError::new(
                self.name(),
                "no chat-mode models parsed; upstream shape may have changed",
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
    fn parses_real_shape_and_converts_per_token_prices() {
        let upstream = json!({
            "azure_ai/deepseek-v3.2": {
                "litellm_provider": "azure_ai",
                "mode": "chat",
                "max_input_tokens": 163840,
                "max_output_tokens": 163840,
                "max_tokens": 163840,
                "input_cost_per_token": 6.2e-07,
                "output_cost_per_token": 1.85e-06,
                "cache_read_input_token_cost": 3.1e-07,
                "supports_function_calling": true,
                "supports_reasoning": true,
                "supports_tool_choice": true,
                "source": "https://azure.microsoft.com/pricing"
            }
        });
        let records = LitellmAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.provider_hint.as_deref(), Some("azure_ai"));
        assert_eq!(r.context_window, Some(163_840));
        assert_eq!(r.capabilities.tool_call, Some(true));
        assert_eq!(r.capabilities.reasoning, Some(true));
        // 每 token → 每百万（实测值 6.2e-07 → 0.62）
        let input = r.pricing.input_per_mtok.unwrap();
        assert!((input - 0.62).abs() < 1e-9, "got {input}");
        let output = r.pricing.output_per_mtok.unwrap();
        assert!((output - 1.85).abs() < 1e-9, "got {output}");
        assert!(r.pricing.cached_input_per_mtok.is_some());
        // 未提供的维度保持 unknown
        assert_eq!(r.pricing.web_search_per_unit, None);
    }

    #[test]
    fn non_chat_modes_are_filtered_out() {
        let upstream = json!({
            "openai/text-embedding-3-small": {"mode": "embedding", "max_input_tokens": 8191},
            "openai/gpt-4o": {"mode": "chat", "max_input_tokens": 128000},
            "openai/dall-e-3": {"mode": "image_generation"}
        });
        let records = LitellmAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "openai/gpt-4o");
    }

    #[test]
    fn duplicate_case_keys_are_preserved_distinct() {
        // 实测该文件真的存在仅大小写不同的键；大小写敏感的 map 必须两者都保留
        let upstream = json!({
            "together_ai/baai/bge-base-en-v1.5": {"mode": "chat", "max_tokens": 512},
            "together_ai/BAAI/bge-base-en-v1.5": {"mode": "chat", "max_tokens": 512}
        });
        let records = LitellmAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records.len(), 2, "大小写不同的键是两个不同记录");
    }

    #[test]
    fn deprecation_date_marks_deprecated() {
        let upstream = json!({
            "old/model": {"mode": "chat", "deprecation_date": "2027-07-01"},
            "new/model": {"mode": "chat"}
        });
        let records = LitellmAdapter::default().parse(&upstream).unwrap();
        let old = records
            .iter()
            .find(|r| r.source_key == "old/model")
            .unwrap();
        let new = records
            .iter()
            .find(|r| r.source_key == "new/model")
            .unwrap();
        assert!(old.deprecated);
        assert!(!new.deprecated);
    }

    #[test]
    fn max_tokens_fallback_when_input_limit_absent() {
        let upstream = json!({"p/m": {"mode": "chat", "max_tokens": 4096}});
        let records = LitellmAdapter::default().parse(&upstream).unwrap();
        assert_eq!(records[0].context_window, Some(4096));
        assert_eq!(records[0].max_output_tokens, Some(4096));
    }

    #[test]
    fn shape_change_is_diagnosable() {
        let err = LitellmAdapter::default()
            .parse(&json!({"a": {"mode": "embedding"}}))
            .unwrap_err();
        assert!(err.reason.contains("no chat-mode models"));
    }
}
