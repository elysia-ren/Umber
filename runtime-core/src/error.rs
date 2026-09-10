//! Canonical Error（总案 §28–§29）。
//!
//! 宿主不应该判断 429 / 400 / context_length_exceeded；
//! 一切错误统一映射到这里，原始信息保留在 detail 中供日志与诊断。

use serde::{Deserialize, Serialize};

/// 四段超时（总案 §31.1）。单一总超时会误杀推理模型，必须细分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutKind {
    /// 建立连接。
    Connect,
    /// 请求发出 → 首个事件（推理模型可达分钟级，默认值必须宽松）。
    FirstToken,
    /// 流中相邻事件的最大间隔（stall 检测）。
    Idle,
    /// 整个 Invocation 的可选上限。
    Total,
}

/// 错误细节。`raw_context` 输出前必须经过凭据脱敏（总案 §28）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_context: Option<serde_json::Value>,
}

impl ErrorDetail {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }
}

/// 统一错误枚举（总案 §28）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelError {
    AuthenticationFailed(ErrorDetail),
    AuthorizationFailed(ErrorDetail),
    RateLimited {
        #[serde(flatten)]
        detail: ErrorDetail,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
    /// Provider 过载（如 Anthropic 529）。
    Overloaded(ErrorDetail),
    ContextExceeded(ErrorDetail),
    InvalidRequest(ErrorDetail),
    Unsupported(ErrorDetail),
    /// 安全拦截 / 拒答。既非 InvalidRequest 也非 ProviderError，宿主需区分。
    ContentFiltered(ErrorDetail),
    Timeout {
        kind: TimeoutKind,
        #[serde(flatten)]
        detail: ErrorDetail,
    },
    NetworkError(ErrorDetail),
    ProviderError {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
        #[serde(flatten)]
        detail: ErrorDetail,
    },
    EmptyResponse,
    Cancelled,
    Unknown(ErrorDetail),
}

impl ModelError {
    /// 默认可重试策略表（总案 §29）。
    ///
    /// `retryable = true` 只表示允许 Runtime 考虑重试，不等于必须重试；
    /// 是否真正重试还取决于 Invocation 状态、部分输出、安全重放能力、
    /// 剩余次数与 Retry-After（§29）。
    pub fn retryable(&self) -> bool {
        match self {
            ModelError::RateLimited { .. }
            | ModelError::Overloaded(_)
            | ModelError::Timeout { .. }
            | ModelError::NetworkError(_) => true,
            ModelError::ProviderError { status, .. } => status.is_some_and(|s| s >= 500),
            // 总案 §29：EmptyResponse 不强制定性；此处取保守默认 false，
            // Runtime 策略层可视 Provider 行为扩展。
            ModelError::EmptyResponse => false,
            _ => false,
        }
    }

    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            ModelError::RateLimited { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        }
    }

    pub fn detail(&self) -> Option<&ErrorDetail> {
        match self {
            ModelError::AuthenticationFailed(d)
            | ModelError::AuthorizationFailed(d)
            | ModelError::Overloaded(d)
            | ModelError::ContextExceeded(d)
            | ModelError::InvalidRequest(d)
            | ModelError::Unsupported(d)
            | ModelError::ContentFiltered(d)
            | ModelError::NetworkError(d)
            | ModelError::Unknown(d) => Some(d),
            ModelError::RateLimited { detail, .. }
            | ModelError::Timeout { detail, .. }
            | ModelError::ProviderError { detail, .. } => Some(detail),
            ModelError::EmptyResponse | ModelError::Cancelled => None,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            ModelError::AuthenticationFailed(_) => "authentication_failed",
            ModelError::AuthorizationFailed(_) => "authorization_failed",
            ModelError::RateLimited { .. } => "rate_limited",
            ModelError::Overloaded(_) => "overloaded",
            ModelError::ContextExceeded(_) => "context_exceeded",
            ModelError::InvalidRequest(_) => "invalid_request",
            ModelError::Unsupported(_) => "unsupported",
            ModelError::ContentFiltered(_) => "content_filtered",
            ModelError::Timeout { .. } => "timeout",
            ModelError::NetworkError(_) => "network_error",
            ModelError::ProviderError { .. } => "provider_error",
            ModelError::EmptyResponse => "empty_response",
            ModelError::Cancelled => "cancelled",
            ModelError::Unknown(_) => "unknown",
        }
    }
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.detail() {
            Some(d) => write!(f, "{}: {}", self.kind_name(), d.message),
            None => f.write_str(self.kind_name()),
        }
    }
}

impl std::error::Error for ModelError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_table_matches_contract() {
        // §29 典型可重试
        assert!(ModelError::RateLimited {
            detail: ErrorDetail::new("x"),
            retry_after_ms: Some(1000),
        }
        .retryable());
        assert!(ModelError::Overloaded(ErrorDetail::new("x")).retryable());
        assert!(ModelError::Timeout {
            kind: TimeoutKind::FirstToken,
            detail: ErrorDetail::new("x"),
        }
        .retryable());
        assert!(ModelError::NetworkError(ErrorDetail::new("x")).retryable());
        assert!(ModelError::ProviderError {
            status: Some(503),
            detail: ErrorDetail::new("x"),
        }
        .retryable());
        // §29 典型不可重试
        assert!(!ModelError::AuthenticationFailed(ErrorDetail::new("x")).retryable());
        assert!(!ModelError::ContextExceeded(ErrorDetail::new("x")).retryable());
        assert!(!ModelError::InvalidRequest(ErrorDetail::new("x")).retryable());
        assert!(!ModelError::ContentFiltered(ErrorDetail::new("x")).retryable());
        assert!(!ModelError::Cancelled.retryable());
        // ProviderError 的 4xx 不可重试
        assert!(!ModelError::ProviderError {
            status: Some(402),
            detail: ErrorDetail::new("x"),
        }
        .retryable());
    }

    #[test]
    fn serializes_with_type_tag_and_flattened_detail() {
        let err = ModelError::RateLimited {
            detail: ErrorDetail {
                message: "slow down".into(),
                provider_code: Some("429".into()),
                ..ErrorDetail::default()
            },
            retry_after_ms: Some(2000),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["type"], "rate_limited");
        assert_eq!(json["message"], "slow down");
        assert_eq!(json["retry_after_ms"], 2000);
        let back: ModelError = serde_json::from_value(json).unwrap();
        assert_eq!(back, err);
    }
}
