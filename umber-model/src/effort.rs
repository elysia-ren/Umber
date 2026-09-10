//! 思考强度档位的归一与就近降级（总案 §21.1）。
//!
//! 档位枚举的唯一定义在 `umber-core::request::ReasoningEffort`
//! （它是 Canonical API 的一部分）；本模块只提供**映射与解析逻辑**，
//! 以自由函数形式存在——Rust 不允许给外部类型加固有方法。
//!
//! 上游数据库的档位词并不统一。实测（2026-09）：
//!
//! ```text
//! OpenRouter   reasoning.supported_efforts = ["max", "high", "low"]
//! models.dev   reasoning_options = [{"type":"toggle"},{"type":"budget_tokens"}]
//! Anthropic    无档位词，只接受 budget_tokens 数值
//! ```
//!
//! 因此 Runtime 必须：
//! 1. 把上游档位词归一为 Canonical 四档
//! 2. 请求档位不被支持时**就近降级**（而不是原样发给 Provider 换回 400）
//! 3. 记录实际生效档位

// 档位枚举的唯一定义在 umber-core（它是 Canonical API 的一部分）；
// 这里公开再导出，让 umber-model 的消费者只需依赖一个 crate。
use serde::{Deserialize, Serialize};
pub use umber_core::request::ReasoningEffort;

/// 上游标签 → Canonical 档位。无法识别返回 `None`（不猜，§X.16）。
///
/// 覆盖实测写法与常见同义写法；OpenRouter 的 `"max"` 就是最高档。
pub fn from_source_label(label: &str) -> Option<ReasoningEffort> {
    match label.trim().to_lowercase().as_str() {
        "minimal" | "none" | "off" | "disabled" => Some(ReasoningEffort::Minimal),
        "low" | "light" | "fast" => Some(ReasoningEffort::Low),
        "medium" | "mid" | "default" | "balanced" => Some(ReasoningEffort::Medium),
        "high" | "max" | "highest" | "xhigh" | "extended" => Some(ReasoningEffort::High),
        _ => None,
    }
}

/// 一批上游标签 → 去重后的受支持档位集合（按强弱排序）。
/// 无法识别的标签被丢弃（不猜），全部无法识别则返回空集。
pub fn efforts_from_labels(labels: &[String]) -> Vec<ReasoningEffort> {
    let mut efforts: Vec<ReasoningEffort> =
        labels.iter().filter_map(|l| from_source_label(l)).collect();
    efforts.sort_unstable();
    efforts.dedup();
    efforts
}

/// Anthropic 的 budget_tokens 映射（总案 §21.1）。
pub fn anthropic_budget_tokens(effort: ReasoningEffort) -> u32 {
    match effort {
        ReasoningEffort::Minimal => 1024,
        ReasoningEffort::Low => 2048,
        ReasoningEffort::Medium => 8192,
        ReasoningEffort::High => 24576,
    }
}

/// Gemini 的 thinkingBudget 映射（总案 §21.1）。
pub fn gemini_thinking_budget(effort: ReasoningEffort) -> i64 {
    match effort {
        ReasoningEffort::Minimal => 0,
        ReasoningEffort::Low => 1024,
        ReasoningEffort::Medium => 8192,
        ReasoningEffort::High => 24576,
    }
}

/// 档位解析结果（总案 §21.1："实际生效档位写入 CompatibilityProfile"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffortResolution {
    /// 实际发送给 Provider 的档位（可能低于请求档位）。
    pub effective: ReasoningEffort,
    /// 请求档位。
    pub requested: ReasoningEffort,
    /// 是否发生了降级。
    pub downgraded: bool,
}

/// 请求档位恰好被支持。
pub fn exact(effort: ReasoningEffort) -> EffortResolution {
    EffortResolution {
        effective: effort,
        requested: effort,
        downgraded: false,
    }
}

/// 模型不支持该档位时的选择（§21.1：就近降级）。
///
/// 策略：优先向下取**最接近**的受支持档位（强度更低但可用）；
/// 若没有更低档则向上取最接近的一档。是"就近"而不是"总是取最低"——
/// 用户要 High 而模型只有 Minimal/Low 时给 Low，不是 Minimal。
///
/// `supported` 为空表示"不知道支持哪些档位"（上游只给了机制没给档位），
/// 此时返回 `None`，调用方应原样发送而不是猜一个档位。
pub fn nearest(
    requested: ReasoningEffort,
    supported: &[ReasoningEffort],
) -> Option<EffortResolution> {
    if supported.is_empty() {
        return None;
    }
    if supported.contains(&requested) {
        return Some(exact(requested));
    }
    let mut below: Vec<ReasoningEffort> = supported
        .iter()
        .copied()
        .filter(|e| *e < requested)
        .collect();
    below.sort_unstable();
    if let Some(effective) = below.last().copied() {
        return Some(EffortResolution {
            effective,
            requested,
            downgraded: true,
        });
    }
    let mut above: Vec<ReasoningEffort> = supported
        .iter()
        .copied()
        .filter(|e| *e > requested)
        .collect();
    above.sort_unstable();
    above.first().copied().map(|effective| EffortResolution {
        effective,
        requested,
        downgraded: true,
    })
}

/// models.dev 只给机制不给档位（实测 `[{"type":"toggle"}]`）——
/// 返回空集表示"档位未知"，调用方不得据此猜档位（§X.16）。
pub fn efforts_from_mechanisms(_types: &[String]) -> Vec<ReasoningEffort> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upstream_labels_normalize() {
        assert_eq!(from_source_label("max"), Some(ReasoningEffort::High));
        assert_eq!(from_source_label("high"), Some(ReasoningEffort::High));
        assert_eq!(from_source_label("low"), Some(ReasoningEffort::Low));
        assert_eq!(
            from_source_label(" MINIMAL "),
            Some(ReasoningEffort::Minimal)
        );
        assert_eq!(from_source_label("medium"), Some(ReasoningEffort::Medium));
        // 不认识的不猜（§X.16）
        assert_eq!(from_source_label("ultra"), None);
        assert_eq!(from_source_label(""), None);
    }

    #[test]
    fn openrouter_label_set_collapses_to_two_efforts() {
        // 实测 OpenRouter: ["max","high","low"] → {Low, High}
        let efforts = efforts_from_labels(&labels(&["max", "high", "low"]));
        assert_eq!(efforts, vec![ReasoningEffort::Low, ReasoningEffort::High]);
    }

    #[test]
    fn unknown_labels_are_dropped_not_guessed() {
        let efforts = efforts_from_labels(&labels(&["high", "ultra", "??"]));
        assert_eq!(efforts, vec![ReasoningEffort::High]);
    }

    #[test]
    fn supported_effort_is_not_downgraded() {
        let r = nearest(
            ReasoningEffort::High,
            &[ReasoningEffort::Low, ReasoningEffort::High],
        )
        .unwrap();
        assert_eq!(r.effective, ReasoningEffort::High);
        assert!(!r.downgraded);
    }

    #[test]
    fn downgrades_to_nearest_below_not_to_minimum() {
        let r = nearest(
            ReasoningEffort::High,
            &[ReasoningEffort::Minimal, ReasoningEffort::Low],
        )
        .unwrap();
        assert_eq!(r.effective, ReasoningEffort::Low, "就近取 Low 而非 Minimal");
        assert_eq!(r.requested, ReasoningEffort::High);
        assert!(r.downgraded);
    }

    #[test]
    fn medium_finds_neighbour_in_openrouter_style_set() {
        let supported = [ReasoningEffort::Low, ReasoningEffort::High];
        let r = nearest(ReasoningEffort::Medium, &supported).unwrap();
        assert_eq!(r.effective, ReasoningEffort::Low);
        assert!(r.downgraded);
    }

    #[test]
    fn downgrades_upward_when_nothing_below() {
        let r = nearest(ReasoningEffort::Minimal, &[ReasoningEffort::Low]).unwrap();
        assert_eq!(r.effective, ReasoningEffort::Low);
        assert!(r.downgraded);
    }

    #[test]
    fn empty_supported_set_means_unknown_not_guess() {
        assert!(nearest(ReasoningEffort::High, &[]).is_none());
    }

    #[test]
    fn protocol_mappings_match_contract_table() {
        assert_eq!(anthropic_budget_tokens(ReasoningEffort::High), 24576);
        assert_eq!(anthropic_budget_tokens(ReasoningEffort::Minimal), 1024);
        assert_eq!(gemini_thinking_budget(ReasoningEffort::Minimal), 0);
        assert_eq!(gemini_thinking_budget(ReasoningEffort::High), 24576);
    }

    #[test]
    fn mechanism_list_does_not_invent_efforts() {
        assert!(efforts_from_mechanisms(&labels(&["toggle", "budget_tokens"])).is_empty());
    }
}
