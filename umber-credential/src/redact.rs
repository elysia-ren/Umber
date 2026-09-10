//! 脱敏工具（总案 §28 §30 §32）。
//!
//! `raw_context` 与一切 Runtime 日志在输出前必须经过这里；
//! 这是强制门禁，不是约定。

use serde_json::{Map, Value};

pub const REDACTED: &str = "[REDACTED]";

/// 键名是否敏感。安全优先：宽松匹配（含 "key" 即视为敏感）。
pub fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_lowercase();
    [
        "api_key",
        "apikey",
        "authorization",
        "token",
        "secret",
        "password",
        "credential",
        "key",
    ]
    .iter()
    .any(|pattern| k.contains(pattern))
}

/// 递归脱敏 JSON：敏感键的字符串值替换为 `[REDACTED]`。
pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    if is_sensitive_key(k) {
                        (k.clone(), Value::from(REDACTED))
                    } else {
                        (k.clone(), redact_json(v))
                    }
                })
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        other => other.clone(),
    }
}

/// 文本脱敏：把出现的已知秘密替换为 `[REDACTED]`。
pub fn redact_text(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret.as_str(), REDACTED);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_sensitive_keys_are_masked() {
        let v = json!({
            "model": "deepseek-chat",
            "headers": {
                "Authorization": "Bearer sk-abc",
                "x-api-key": "sk-xyz",
                "content-type": "application/json"
            },
            "body": { "messages": [] }
        });
        let red = redact_json(&v);
        assert_eq!(red["headers"]["Authorization"], REDACTED);
        assert_eq!(red["headers"]["x-api-key"], REDACTED);
        assert_eq!(red["headers"]["content-type"], "application/json");
        assert_eq!(red["model"], "deepseek-chat");
        // 原值不被修改
        assert_eq!(v["headers"]["Authorization"], "Bearer sk-abc");
    }

    #[test]
    fn text_secrets_are_replaced() {
        let out = redact_text(
            "request used sk-abc123 at edge",
            &[format!("sk-abc{}", "123")],
        );
        assert_eq!(out, "request used [REDACTED] at edge");
    }
}
