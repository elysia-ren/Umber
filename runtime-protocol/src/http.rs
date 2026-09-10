//! 真实 HTTP/SSE 传输（总案 §39.4：I/O 细节归传输层，协议转换归 Adapter）。
//!
//! 实现要点：
//! - 阻塞式 `ureq`，与 Engine 的拉取式模型天然对应
//! - **读超时是取消与 stall 的最终保障**：`ProviderStream::next_event` 的
//!   deadline 无法打断已阻塞的 socket 读，因此这里在 agent 上设置读超时
//! - 代理：优先显式配置，其次 `ALL_PROXY` / `HTTPS_PROXY` / `HTTP_PROXY`
//! - 非 2xx 不作为传输错误抛出，而是交给协议层做 Canonical 错误映射
//! - 凭据不进日志（调用方对 raw_context 再脱敏，§28）

use std::io::{BufRead, BufReader};
use std::time::Duration;

use runtime_core::error::{ErrorDetail, ModelError, TimeoutKind};
use ureq::{Agent, AgentBuilder, Proxy, Response};

use crate::transport::{Headers, HttpTransport, HttpResponse};

/// 真实传输的配置。
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// 建立连接超时（对应四段超时的 connect，总案 §31.1）。
    pub connect_timeout: Duration,
    /// 读超时上限。取消与 stall 检测的最终保障。
    pub read_timeout: Duration,
    /// 代理地址；`None` 时读取环境变量。
    pub proxy: Option<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(120),
            proxy: None,
        }
    }
}

/// 传输错误信息中出现的超时关键词（跨 ureq 版本稳定）。
const TIMEOUT_HINTS: &[&str] = &["timed out", "timeout", "deadline"];

pub struct RealHttpTransport {
    agent: Agent,
}

impl RealHttpTransport {
    pub fn new(config: HttpConfig) -> Self {
        let mut builder = AgentBuilder::new()
            .timeout_connect(config.connect_timeout)
            .timeout_read(config.read_timeout);

        // 代理优先取显式配置，其次环境变量，最后系统代理（Windows/macOS）
        if let Some(p) = resolve_proxy(config.proxy) {
            if let Ok(proxy) = Proxy::new(&p) {
                builder = builder.proxy(proxy);
            }
        }

        Self {
            agent: builder.build(),
        }
    }

    /// 发送请求。非 2xx 状态不抛错——返回响应体供协议层映射为 Canonical 错误。
    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &Headers,
        body: Option<String>,
    ) -> Result<Response, ModelError> {
        let request = if method.eq_ignore_ascii_case("GET") {
            self.agent.get(url)
        } else {
            self.agent.post(url)
        };
        let mut request = request;
        for (name, value) in headers {
            request = request.set(name.as_str(), value.as_str());
        }
        let outcome = match body {
            Some(b) => request.send_string(&b),
            None => request.call(),
        };
        match outcome {
            Ok(response) => Ok(response),
            // ureq 2.x：非 2xx 走 Status 分支，仍带完整响应体
            Err(ureq::Error::Status(_, response)) => Ok(response),
            Err(ureq::Error::Transport(t)) => Err(map_transport(&t)),
        }
    }
}

/// 解析代理来源，优先级：显式配置 → 环境变量 → 系统代理。
///
/// **系统代理是桌面场景的现实需求**：Windows 用户在"Internet 选项"里
/// 打开代理（如 127.0.0.1:7897）时不会设置环境变量，而 ureq 只读环境变量。
/// 不读系统代理，宿主在用户机器上会莫名其妙地连不上。
fn resolve_proxy(explicit: Option<String>) -> Option<String> {
    explicit.or_else(env_proxy).or_else(system_proxy)
}

/// 环境变量代理（大小写两种写法都覆盖）。
fn env_proxy() -> Option<String> {
    ["ALL_PROXY", "all_proxy", "HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .filter(|v| !v.trim().is_empty())
}

/// 读取操作系统代理设置。
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_READ,
        )
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    normalize_proxy_server(&server)
}

#[cfg(target_os = "macos")]
fn system_proxy() -> Option<String> {
    let output = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut host = None;
    let mut port = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("HTTPSProxy : ") {
            host = Some(v.trim().to_string());
        }
        if let Some(v) = line.strip_prefix("HTTPSPort : ") {
            port = Some(v.trim().to_string());
        }
    }
    match (host, port) {
        (Some(h), Some(p)) => Some(format!("http://{h}:{p}")),
        _ => None,
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_proxy() -> Option<String> {
    None
}

/// Windows 的 `ProxyServer` 有两种写法：
/// `127.0.0.1:7897` 或 `http=host:port;https=host:port`。
fn normalize_proxy_server(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if server.contains('=') {
        // 按协议分配形式：优先 https，其次 http
        let mut https = None;
        let mut http = None;
        for part in server.split(';') {
            let Some((scheme, addr)) = part.split_once('=') else {
                continue;
            };
            match scheme.trim().to_lowercase().as_str() {
                "https" => https = Some(addr.trim().to_string()),
                "http" => http = Some(addr.trim().to_string()),
                _ => {}
            }
        }
        let addr = https.or(http)?;
        return Some(with_scheme(&addr));
    }
    Some(with_scheme(server))
}

fn with_scheme(addr: &str) -> String {
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    }
}

fn map_transport(t: &ureq::Transport) -> ModelError {
    let message = t.to_string();
    let lower = message.to_lowercase();
    if TIMEOUT_HINTS.iter().any(|h| lower.contains(h)) {
        ModelError::Timeout {
            kind: TimeoutKind::Connect, // Engine 按阶段重标（§31.1）
            detail: ErrorDetail::new(message),
        }
    } else {
        ModelError::NetworkError(ErrorDetail::new(message))
    }
}

/// 响应体内容。
enum BodyContent {
    Json(serde_json::Value),
    Text(String),
}

/// 读完整响应体。
///
/// **真实世界发现**：并非所有 Provider 的错误体都是 JSON——
/// DeepSeek 无凭据时返回纯文本 `Authentication Fails (governor)`。
/// 因此非 JSON 体必须原样保留为 `Text`，由协议层统一走
/// `map_status(status, None, body)`，而不是在传输层报错。
fn read_body(response: Response) -> Result<(u16, BodyContent), ModelError> {
    let status = response.status();
    let text = response
        .into_string()
        .map_err(|e| ModelError::NetworkError(ErrorDetail::new(format!("read body: {e}"))))?;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(json) => Ok((status, BodyContent::Json(json))),
        Err(_) => Ok((status, BodyContent::Text(text))),
    }
}

fn into_response(status: u16, content: BodyContent) -> HttpResponse {
    match content {
        BodyContent::Json(body) => HttpResponse::Json { status, body },
        BodyContent::Text(body) => HttpResponse::Text { status, body },
    }
}

impl HttpTransport for RealHttpTransport {
    fn post_stream(
        &self,
        url: &str,
        headers: &Headers,
        body: String,
    ) -> Result<HttpResponse, ModelError> {
        let response = self.send("POST", url, headers, Some(body))?;
        let status = response.status();
        if !(200..300).contains(&status) {
            let (status, content) = read_body(response)?;
            return Ok(into_response(status, content));
        }
        // SSE：按行拉取。每次 socket 读受 agent 的读超时保护，
        // 取消由 Engine 的拉取分片 + worker 的取消检查保证（§31）。
        let reader = BufReader::new(response.into_reader());
        let lines = reader.lines().map(|line| match line {
            Ok(l) => Ok(l),
            Err(e) => Err(ModelError::NetworkError(ErrorDetail::new(format!(
                "stream read failed: {e}"
            )))),
        });
        Ok(HttpResponse::Sse {
            lines: Box::new(lines),
        })
    }

    fn get(&self, url: &str, headers: &Headers) -> Result<HttpResponse, ModelError> {
        let response = self.send("GET", url, headers, None)?;
        let (status, content) = read_body(response)?;
        Ok(into_response(status, content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_constructs_with_and_without_proxy() {
        let _ = RealHttpTransport::new(HttpConfig::default());
        let _ = RealHttpTransport::new(HttpConfig {
            proxy: Some("http://127.0.0.1:7890".into()),
            ..HttpConfig::default()
        });
        // 无法解析的代理不应 panic（回落到直连）
        let _ = RealHttpTransport::new(HttpConfig {
            proxy: Some("not a url".into()),
            ..HttpConfig::default()
        });
    }

    #[test]
    fn proxy_server_forms_are_normalized() {
        assert_eq!(
            normalize_proxy_server("127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            normalize_proxy_server("http=127.0.0.1:8080;https=127.0.0.1:8443").as_deref(),
            Some("http://127.0.0.1:8443")
        );
        assert_eq!(
            normalize_proxy_server("http=127.0.0.1:8080").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(normalize_proxy_server("   ").as_deref(), None);
        assert_eq!(
            normalize_proxy_server("https://host:1080").as_deref(),
            Some("https://host:1080")
        );
    }

    #[test]
    fn explicit_proxy_wins_over_system() {
        // 显式值必须原样通过，不受系统设置影响
        assert_eq!(
            resolve_proxy(Some("http://explicit:1".into())).as_deref(),
            Some("http://explicit:1")
        );
    }

    #[test]
    fn timeout_transport_error_maps_to_timeout() {
        let t = ureq::Error::from(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "operation timed out",
        ));
        let t = match t {
            ureq::Error::Transport(t) => t,
            other => panic!("expected transport error, got {other:?}"),
        };
        let err = map_transport(&t);
        assert!(matches!(err, ModelError::Timeout { .. }));
        assert!(err.retryable());
    }

    #[test]
    fn other_transport_error_maps_to_network_error() {
        let t = ureq::Error::from(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset by peer",
        ));
        let t = match t {
            ureq::Error::Transport(t) => t,
            other => panic!("expected transport error, got {other:?}"),
        };
        let err = map_transport(&t);
        assert!(matches!(err, ModelError::NetworkError(_)));
        assert!(err.retryable());
    }
}
