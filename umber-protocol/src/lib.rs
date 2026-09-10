//! 协议 Adapter 层（总案 §4–§5，开发计划 M2–M5）。
//!
//! 四协议一次给全，共享同一套基建：
//! - [`transport`]：HTTP 传输抽象（真实网络实现待联网环境接入，见下）
//! - [`scripted`]：可脚本化传输，conformance 的唯一考场来源
//! - [`sse`]：SSE 事件流装配（OpenAI 无事件名 / Anthropic / Gemini 有）
//! - [`worker`]：读线程 → 通道 → 拉取式 `ProviderStream`（取消响应依赖
//!   Engine 的分片拉取；读阻塞上限由传输层读超时决定）
//! - [`errors`]：状态码 → `ModelError` 的通用映射表，各协议在其上细化
//!
//! 协议无关的约定：
//! - URL 拼接：Endpoint.url 是 base；若已含已知动作后缀则原样使用，
//!   否则追加协议动作路径（总案 §34 Endpoint 可覆盖的延伸）
//! - 终结事件由 Adapter 解析协议完成信号后合成 `Completed`；
//!   提前 EOF / 断流交给 Engine 合成 `Failed`（§23.1）

#![forbid(unsafe_code)]

pub mod anthropic;
pub mod errors;
pub mod gemini;
pub mod http;
pub mod jsonh;
pub mod openai_chat;
pub mod openai_responses;
pub mod scripted;
pub mod sse;
pub mod transport;
pub mod worker;

pub use anthropic::AnthropicAdapter;
pub use gemini::GeminiAdapter;
pub use http::{HttpConfig, RealHttpTransport};
pub use openai_chat::OpenAiChatAdapter;
pub use openai_responses::OpenAiResponsesAdapter;
pub use scripted::ScriptedTransport;
pub use transport::{HttpResponse, HttpTransport};

/// URL 拼接：base + 动作路径；base 已含动作后缀则原样返回。
pub fn join_url(base: &str, action: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with(action) {
        return base.to_string();
    }
    format!("{base}/{action}")
}

/// 去掉 base 末尾的动作后缀（用于 Gemini 的模型路径拼接）。
pub fn trim_trailing_action<'a>(base: &'a str, action: &str) -> &'a str {
    base.trim_end_matches('/')
        .strip_suffix(action)
        .unwrap_or_else(|| base.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_appends_action_once() {
        assert_eq!(
            join_url("https://api.deepseek.com/v1", "chat/completions"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            join_url("https://gw.example/v1/chat/completions", "chat/completions"),
            "https://gw.example/v1/chat/completions"
        );
    }

    #[test]
    fn trim_action_for_model_paths() {
        assert_eq!(
            trim_trailing_action("https://g.example/v1beta/", ""),
            "https://g.example/v1beta"
        );
    }
}
