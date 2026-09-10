//! 统一 Usage（总案 §45）。
//!
//! Canonical 只承诺这里的统一字段；Provider 返回的额外字段全部进入
//! `provider_usage`，既不丢信息，也不污染核心类型。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    /// Provider 原始 usage，Runtime 不解释（总案 §43）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_usage: Option<serde_json::Value>,
}

impl Usage {
    /// `total_tokens = input + output`；`reasoning_tokens` 通常已含于 output、
    /// `cached_input_tokens` 已含于 input（口径差异见 provider_usage）。
    pub fn new(
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        cached_input_tokens: u64,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            reasoning_tokens,
            cached_input_tokens,
            total_tokens: input_tokens.saturating_add(output_tokens),
            provider_usage: None,
        }
    }

    pub fn zero() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_is_input_plus_output() {
        let usage = Usage::new(100, 50, 20, 30);
        assert_eq!(usage.total_tokens, 150);
    }
}
