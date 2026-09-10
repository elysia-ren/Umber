//! Source Adapter 层（规格 §2：每个上游数据库一个适配器）。
//!
//! 每个适配器只负责 **上游结构 → `RawModelRecord`**，不解释档位语义、
//! 不做身份合并、不裁决冲突。这些都在 pipeline 里统一处理。
//!
//! 实测的上游形状（2026-09）：
//!
//! ```text
//! models.dev   { provider: { id, name, models: { "provider/model": {...} } } }
//! LiteLLM      { "provider/model": { litellm_provider, max_input_tokens, ... } }
//! OpenRouter   { data: [ { id, architecture, pricing, supported_parameters, reasoning } ] }
//! ```

pub mod litellm;
pub mod models_dev;
pub mod official;
pub mod openrouter;

pub use litellm::LitellmAdapter;
pub use models_dev::ModelsDevAdapter;
pub use official::OfficialOverlay;
pub use openrouter::OpenRouterAdapter;

use crate::record::RawModelRecord;

/// Source Adapter 契约：把上游 JSON 解析为统一中间结构。
pub trait SourceAdapter {
    /// 上游数据库名（用于 Evidence 来源标识与许可证清单）。
    fn name(&self) -> &'static str;
    /// 上游快照标识（版本 / 日期 / commit），用于可复现构建。
    fn snapshot(&self) -> String;
    /// 解析。
    fn parse(&self, upstream: &serde_json::Value) -> Result<Vec<RawModelRecord>, SourceError>;
}

/// 适配器错误。上游结构变化是**常态**，因此错误必须可诊断而不是 panic。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub source: &'static str,
    pub reason: String,
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source, self.reason)
    }
}

impl std::error::Error for SourceError {}

impl SourceError {
    pub fn new(source: &'static str, reason: impl Into<String>) -> Self {
        Self {
            source,
            reason: reason.into(),
        }
    }
}

/// 当前 Unix 秒。
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 上游价格值统一转 f64：既接受数字也接受字符串（OpenRouter 用字符串）。
pub fn price_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn price_accepts_both_number_and_string_forms() {
        // LiteLLM 用数字，OpenRouter 用字符串——两种实测形态都必须支持
        assert_eq!(price_f64(&json!(6.2e-07)), Some(6.2e-07));
        assert_eq!(price_f64(&json!("0.0000003")), Some(0.0000003));
        assert_eq!(price_f64(&json!(null)), None);
        assert_eq!(price_f64(&json!("n/a")), None);
        assert_eq!(price_f64(&json!({})), None);
    }
}
