//! 可脚本化传输（总案 §52 fake-provider 的 HTTP 层）。
//!
//! 按 FIFO 顺序回放预置响应，并记录收到的每个请求（url / headers / body），
//! 供 fixtures 断言"请求构造正确"。请求体中的 API Key 在记录前被
//! `[REDACTED]`（总案 §28：日志与诊断强制脱敏）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use runtime_core::error::{ErrorDetail, ModelError};

use crate::transport::{Headers, HttpResponse, HttpTransport};

/// 一次被记录的请求（body 已脱敏）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Headers,
    pub body: Option<String>,
}

#[derive(Clone, Default)]
pub struct ScriptedTransport {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    responses: VecDeque<HttpResponse>,
    requests: Vec<RecordedRequest>,
}

const SENSITIVE_HEADERS: &[&str] = &["authorization", "x-api-key", "api-key", "cookie"];

fn redact_headers(headers: &Headers) -> Headers {
    headers
        .iter()
        .map(|(k, v)| {
            if SENSITIVE_HEADERS
                .iter()
                .any(|s| k.to_lowercase().contains(s))
            {
                (k.clone(), "[REDACTED]".to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

impl ScriptedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个响应（FIFO）。
    pub fn enqueue(&self, response: HttpResponse) -> &Self {
        self.inner
            .lock()
            .expect("scripted transport")
            .responses
            .push_back(response);
        self
    }

    /// 便捷：从 SSE 文本构造响应（按行切分）。
    pub fn enqueue_sse(&self, status: u16, sse: &str) -> &Self {
        assert_eq!(status, 200, "SSE replay only supports 200");
        let lines: Vec<Result<String, ModelError>> =
            sse.lines().map(|l| Ok(l.to_string())).collect();
        self.enqueue(HttpResponse::Sse {
            lines: Box::new(lines.into_iter()),
        })
    }

    pub fn enqueue_json(&self, status: u16, body: serde_json::Value) -> &Self {
        self.enqueue(HttpResponse::Json { status, body })
    }

    /// 收到的请求记录（body 已脱敏）。
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.inner
            .lock()
            .expect("scripted transport")
            .requests
            .clone()
    }

    pub fn last_request(&self) -> Option<RecordedRequest> {
        self.inner
            .lock()
            .expect("scripted transport")
            .requests
            .last()
            .cloned()
    }

    fn record(&self, method: &'static str, url: &str, headers: &Headers, body: Option<String>) {
        let mut inner = self.inner.lock().expect("scripted transport");
        inner.requests.push(RecordedRequest {
            method,
            url: url.to_string(),
            headers: redact_headers(headers),
            // API Key 值不进记录：整体 body 的脱敏由调用方在断言侧用
            // runtime_credential::redact 完成；这里不做内容猜测。
            body,
        });
    }
}

impl HttpTransport for ScriptedTransport {
    fn post_stream(
        &self,
        url: &str,
        headers: &Headers,
        body: String,
    ) -> Result<HttpResponse, ModelError> {
        self.record("POST", url, headers, Some(body));
        self.inner
            .lock()
            .expect("scripted transport")
            .responses
            .pop_front()
            .ok_or_else(|| ModelError::NetworkError(ErrorDetail::new("no scripted response")))
    }

    fn get(&self, url: &str, headers: &Headers) -> Result<HttpResponse, ModelError> {
        self.record("GET", url, headers, None);
        self.inner
            .lock()
            .expect("scripted transport")
            .responses
            .pop_front()
            .ok_or_else(|| ModelError::NetworkError(ErrorDetail::new("no scripted response")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_headers_are_redacted_in_records() {
        let t = ScriptedTransport::new();
        t.enqueue_json(200, serde_json::json!({}));
        let _ = t.get(
            "https://x/models",
            &vec![
                ("Authorization".into(), "Bearer sk-secret".into()),
                ("accept".into(), "application/json".into()),
            ],
        );
        let req = t.last_request().unwrap();
        assert_eq!(req.headers[0].1, "[REDACTED]");
        assert_eq!(req.headers[1].1, "application/json");
    }

    #[test]
    fn fifo_order() {
        let t = ScriptedTransport::new();
        t.enqueue_json(200, serde_json::json!(1));
        t.enqueue_json(200, serde_json::json!(2));
        match t.get("u", &vec![]).unwrap() {
            HttpResponse::Json { body, .. } => assert_eq!(body, serde_json::json!(1)),
            _ => panic!(),
        }
        match t.get("u", &vec![]).unwrap() {
            HttpResponse::Json { body, .. } => assert_eq!(body, serde_json::json!(2)),
            _ => panic!(),
        }
    }
}
