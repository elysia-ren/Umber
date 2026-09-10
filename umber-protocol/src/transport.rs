//! HTTP 传输抽象（总案 §39.4：协议转换归 Adapter，I/O 细节归传输层）。
//!
//! 真实网络实现（TLS、代理、读超时）在具备联网与凭据的环境接入；
//! 本 trait 同时是 conformance 的接缝——`ScriptedTransport` 实现它，
//! 所有协议 Adapter 的 fixtures 都经由同一契约。

use umber_core::error::ModelError;

pub type Headers = Vec<(String, String)>;

/// 一次 HTTP 调用的结果。
pub enum HttpResponse {
    /// 2xx 且为事件流（SSE）。按行迭代；行尾不含换行符。
    Sse {
        lines: Box<dyn Iterator<Item = Result<String, ModelError>> + Send>,
    },
    /// 非 2xx 或普通 JSON 响应。
    Json {
        status: u16,
        body: serde_json::Value,
    },
    /// 非 2xx 且非 JSON（罕见），保留原文供诊断（已脱敏责任在传输层）。
    Text { status: u16, body: String },
}

/// SSE 行迭代器不可格式化，Debug 只呈现形状与状态码（诊断够用）。
impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpResponse::Sse { .. } => f.write_str("HttpResponse::Sse { .. }"),
            HttpResponse::Json { status, body } => f
                .debug_struct("HttpResponse::Json")
                .field("status", status)
                .field("body", body)
                .finish(),
            HttpResponse::Text { status, body } => f
                .debug_struct("HttpResponse::Text")
                .field("status", status)
                .field("body_len", &body.len())
                .finish(),
        }
    }
}

/// 阻塞式 HTTP 传输。
pub trait HttpTransport: Send + Sync {
    /// POST 并期待事件流。
    fn post_stream(
        &self,
        url: &str,
        headers: &Headers,
        body: String,
    ) -> Result<HttpResponse, ModelError>;

    /// GET 资源（模型发现等）。
    fn get(&self, url: &str, headers: &Headers) -> Result<HttpResponse, ModelError>;
}
