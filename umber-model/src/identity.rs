//! Model Identity（总案 §9）与身份匹配边界（§9.1）。

use serde::{Deserialize, Serialize};

/// 统一模型身份（总案 §9；规格 X.1）。
///
/// 铁律：Identity 相同 ≠ Deployment 能力完全相同，因此必须保留 Deployment。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub canonical_id: String,
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 模型的归属组织（规格 X.1 的 organization，如 "deepseek-ai"）。
    /// 与 provider 不同：provider 是"谁提供服务"，organization 是"谁做的模型"。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// 身份匹配前的规范化（总案 §9.1）：小写、去首尾空白。
///
/// 只做窄而可靠的匹配：人工审核 alias 表 + 规范化精确匹配。
/// 不做运行时模糊合并——错误的合并会污染能力与价格数据，
/// 宁可身份偏多，不可错误合并。
pub fn normalize_model_id(raw: &str) -> String {
    raw.trim().to_lowercase()
}

impl ModelIdentity {
    /// 规范化精确匹配：canonical_id 或任一人工审核 alias。
    pub fn matches(&self, raw: &str) -> bool {
        let needle = normalize_model_id(raw);
        if needle.is_empty() {
            return false;
        }
        normalize_model_id(&self.canonical_id) == needle
            || self.aliases.iter().any(|a| normalize_model_id(a) == needle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ModelIdentity {
        ModelIdentity {
            canonical_id: "official/model-x".into(),
            family: "model-x".into(),
            version: None,
            organization: Some("example".into()),
            aliases: vec!["gateway/model-x".into(), "Model-X".into()],
        }
    }

    #[test]
    fn matches_are_exact_after_normalization() {
        let id = identity();
        assert!(id.matches("official/model-x"));
        assert!(id.matches("  MODEL-X  "));
        assert!(id.matches("gateway/model-x"));
    }

    #[test]
    fn no_fuzzy_matching() {
        let id = identity();
        // 前缀 / 子串 / 近似都不算匹配
        assert!(!id.matches("official/model-xl"));
        assert!(!id.matches("model"));
        assert!(!id.matches("x"));
        assert!(!id.matches(""));
    }
}
