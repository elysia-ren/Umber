//! Retry 决策（总案 §29–§30）。
//!
//! 铁律：`retryable = true` 只表示允许重试，不等于必须重试。
//! 真正决策由本模块给出，输入还包括：是否已产生部分输出（已产出绝不盲重）
//! 与 Retry-After。重试执行归 Runtime Core，Adapter 无权实现。

use std::time::Duration;

use runtime_core::ModelError;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 最大重试次数（不含首次尝试）。
    pub max_retries: u32,
    /// 指数退避基数。
    pub base_backoff: Duration,
    /// 退避上限。
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            base_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    Retry { wait: Duration },
    GiveUp,
}

/// 对一次失败做重试决策。
///
/// - 已产生部分输出 → 永不重放（总案 §29：不能把两次结果拼接）
/// - 不可重试错误 → GiveUp
/// - 重试次数用尽 → GiveUp
/// - 等待时间：优先 Retry-After，否则指数退避，封顶 `max_backoff`
pub fn decide(
    policy: &RetryPolicy,
    attempts_used: u32,
    error: &ModelError,
    emitted_content: bool,
) -> RetryDecision {
    if emitted_content {
        return RetryDecision::GiveUp;
    }
    if !error.retryable() {
        return RetryDecision::GiveUp;
    }
    if attempts_used >= policy.max_retries {
        return RetryDecision::GiveUp;
    }
    let attempt_index = attempts_used; // 第 attempts_used+1 次尝试前的退避
    let backoff = policy
        .base_backoff
        .saturating_mul(2u32.saturating_pow(attempt_index))
        .min(policy.max_backoff);
    let wait = error
        .retry_after_ms()
        .map(Duration::from_millis)
        .unwrap_or(backoff);
    RetryDecision::Retry { wait }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::error::ErrorDetail;

    fn rate_limited(after_ms: Option<u64>) -> ModelError {
        ModelError::RateLimited {
            detail: ErrorDetail::new("x"),
            retry_after_ms: after_ms,
        }
    }

    #[test]
    fn partial_output_never_replays() {
        let d = decide(&RetryPolicy::default(), 0, &rate_limited(None), true);
        assert_eq!(d, RetryDecision::GiveUp);
    }

    #[test]
    fn non_retryable_gives_up() {
        let d = decide(
            &RetryPolicy::default(),
            0,
            &ModelError::AuthenticationFailed(ErrorDetail::new("x")),
            false,
        );
        assert_eq!(d, RetryDecision::GiveUp);
    }

    #[test]
    fn retries_exhaust() {
        let d = decide(&RetryPolicy::default(), 2, &rate_limited(None), false);
        assert_eq!(d, RetryDecision::GiveUp);
        let d = decide(&RetryPolicy::default(), 1, &rate_limited(None), false);
        assert!(matches!(d, RetryDecision::Retry { .. }));
    }

    #[test]
    fn retry_after_wins_over_backoff() {
        let d = decide(&RetryPolicy::default(), 0, &rate_limited(Some(250)), false);
        assert_eq!(
            d,
            RetryDecision::Retry {
                wait: Duration::from_millis(250)
            }
        );
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let policy = RetryPolicy {
            max_retries: 10,
            base_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
        };
        assert_eq!(
            decide(&policy, 0, &rate_limited(None), false),
            RetryDecision::Retry {
                wait: Duration::from_millis(500)
            }
        );
        match decide(&policy, 5, &rate_limited(None), false) {
            RetryDecision::Retry { wait } => assert_eq!(wait, Duration::from_secs(8)),
            _ => panic!("expected retry"),
        }
    }
}
