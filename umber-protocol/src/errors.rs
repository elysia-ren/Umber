//! 状态码 → `ModelError` 的通用映射（总案 §28）。
//!
//! 各协议先从错误体提取 provider_code / provider_message 细化，
//! 再落到本表；表本身是协议无关的兜底。

use umber_core::error::{ErrorDetail, ModelError};

pub fn detail(message: impl Into<String>) -> ErrorDetail {
    ErrorDetail::new(message)
}

/// 通用 HTTP 状态兜底映射。`provider_code` / `provider_message` 已由
/// 协议层从错误体提取。
pub fn map_status(status: u16, provider_code: Option<String>, message: String) -> ModelError {
    let mut d = detail(message);
    d.provider_code = provider_code;
    match status {
        401 => ModelError::AuthenticationFailed(d),
        402 | 403 => ModelError::AuthorizationFailed(d),
        404 => ModelError::InvalidRequest(d),
        408 => ModelError::Timeout {
            kind: umber_core::error::TimeoutKind::Connect,
            detail: d,
        },
        409 => ModelError::InvalidRequest(d),
        413 => ModelError::ContextExceeded(d),
        422 | 400 => ModelError::InvalidRequest(d),
        429 => ModelError::RateLimited {
            detail: d,
            retry_after_ms: None,
        },
        s if (500..600).contains(&s) => ModelError::ProviderError {
            status: Some(s),
            detail: d,
        },
        _ => ModelError::Unknown(d),
    }
}

/// 从错误 JSON 体提取 (code, message)。兼容多种常见形状：
/// `{"error":{...}}` / `{"error":"str"}` / 顶层 `message` / `{"code":..}`。
pub fn extract_code_message(body: &serde_json::Value) -> (Option<String>, String) {
    let mut code = None;
    let mut message = String::new();

    let err = body.get("error");
    match err {
        Some(serde_json::Value::Object(o)) => {
            code = o.get("code").and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|u| u.to_string()))
            });
            if let Some(t) = o.get("type").and_then(|v| v.as_str()) {
                code = code.or_else(|| Some(t.to_string()));
            }
            if let Some(m) = o.get("message").and_then(|v| v.as_str()) {
                message = m.to_string();
            }
        }
        Some(serde_json::Value::String(s)) => {
            message = s.clone();
        }
        _ => {}
    }
    if message.is_empty() {
        if let Some(m) = body.get("message").and_then(|v| v.as_str()) {
            message = m.to_string();
        }
    }
    if message.is_empty() {
        message = format!("provider returned error body: {body}");
    }
    (code, message)
}

/// 已知 provider_code / message 子串的语义修正（在 map_status 之后调用）。
pub fn refine(error: ModelError, provider_code: Option<&str>, message: &str) -> ModelError {
    let lower = message.to_lowercase();
    let code = provider_code.unwrap_or("");
    // 上下文超长的各种说法
    if lower.contains("context length")
        || lower.contains("context_length")
        || lower.contains("prompt is too long")
        || lower.contains("maximum context length")
        || code.contains("context_length")
    {
        return ModelError::ContextExceeded(detail(message));
    }
    match error {
        // 配额耗尽是授权/余额问题，不是限速
        ModelError::RateLimited { .. }
            if code == "insufficient_quota" || code == "insufficient_balance" =>
        {
            ModelError::AuthorizationFailed(detail(message))
        }
        // Anthropic 529 overloaded_error
        ModelError::ProviderError { detail: d, .. } if code == "overloaded_error" => {
            ModelError::Overloaded(ErrorDetail {
                provider_code: Some(code.to_string()),
                ..d
            })
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_table() {
        assert!(matches!(
            map_status(401, None, "no key".into()),
            ModelError::AuthenticationFailed(_)
        ));
        assert!(matches!(
            map_status(429, None, "slow".into()),
            ModelError::RateLimited { .. }
        ));
        assert!(matches!(
            map_status(503, None, "down".into()),
            ModelError::ProviderError {
                status: Some(503),
                ..
            }
        ));
    }

    #[test]
    fn extracts_from_openai_and_anthropic_shapes() {
        let (code, msg) = extract_code_message(&serde_json::json!({
            "error": {"message": "boom", "type": "invalid_request_error", "code": "ctx"}
        }));
        assert_eq!(code.as_deref(), Some("ctx"));
        assert_eq!(msg, "boom");

        let (code, msg) = extract_code_message(&serde_json::json!({
            "error": {"type": "overloaded_error", "message": "overloaded"}
        }));
        assert_eq!(code.as_deref(), Some("overloaded_error"));
        assert_eq!(msg, "overloaded");
    }

    #[test]
    fn refine_detects_context_overflow() {
        let base = map_status(
            400,
            Some("bad".into()),
            "maximum context length exceeded".into(),
        );
        assert!(matches!(
            refine(base, Some("bad"), "maximum context length exceeded"),
            ModelError::ContextExceeded(_)
        ));
    }

    #[test]
    fn refine_quota_to_authorization() {
        let base = map_status(429, Some("insufficient_quota".into()), "quota".into());
        assert!(matches!(
            refine(base, Some("insufficient_quota"), "quota"),
            ModelError::AuthorizationFailed(_)
        ));
    }
}
