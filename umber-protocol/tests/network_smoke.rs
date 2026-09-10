//! 真实网络冒烟（人工门禁，默认忽略；计划中"每个协议一次真实冒烟"）。
//!
//! ```text
//! cargo test -p umber-protocol --test network_smoke -- --ignored --nocapture
//! ```
//!
//! 两档验证：
//! 1. **无需凭据**：对四家官方端点发真实 TLS 请求，断言拿到的是
//!    Canonical 错误（401/403 → AuthenticationFailed/AuthorizationFailed），
//!    而不是传输层失败——这证明 DNS / TLS / HTTP / 错误映射整条链路是通的。
//! 2. **有凭据**（环境变量）：完成一次真实流式调用，断言事件流满足
//!    终结保证与序列约束。
//!
//! 环境变量（存在哪一个就测哪一个）：
//! ```text
//! UMER_SMOKE_DEEPSEEK_KEY     + 可选 UMER_SMOKE_DEEPSEEK_MODEL（默认 deepseek-chat）
//! UMER_SMOKE_OPENAI_KEY       + 可选 UMER_SMOKE_OPENAI_MODEL（默认 gpt-4o-mini）
//! UMER_SMOKE_ANTHROPIC_KEY    + 可选 UMER_SMOKE_ANTHROPIC_MODEL（默认 claude-3-5-haiku-latest）
//! UMER_SMOKE_GEMINI_KEY       + 可选 UMER_SMOKE_GEMINI_MODEL（默认 gemini-2.5-flash）
//! ```

use std::sync::Arc;

use umber_conformance::assert as cassert;
use umber_core::error::ModelError;
use umber_core::event::SequencedEvent;
use umber_core::message::Message;
use umber_core::request::GenerateRequest;
use umber_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use umber_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use umber_model::deployment::{Deployment, Endpoint, ProtocolKind};
use umber_protocol::{
    AnthropicAdapter, GeminiAdapter, HttpConfig, OpenAiChatAdapter, OpenAiResponsesAdapter,
    RealHttpTransport, ScriptedTransport,
};
use umber_provider::ProviderAdapter;

const KEY_REF: &str = "smoke/api_key";

fn transport() -> Arc<RealHttpTransport> {
    Arc::new(RealHttpTransport::new(HttpConfig {
        connect_timeout: std::time::Duration::from_secs(15),
        read_timeout: std::time::Duration::from_secs(120),
        proxy: None, // 自动读取环境变量
    }))
}

fn store_with(key: &str) -> InMemoryCredentialStore {
    let store = InMemoryCredentialStore::new();
    store
        .set(&CredentialRef::from(KEY_REF), SecretString::new(key))
        .unwrap();
    store
}

fn endpoint(url: &str, provider: &str) -> Endpoint {
    Endpoint {
        id: "smoke-ep".into(),
        provider_id: provider.into(),
        url: url.into(),
    }
}

fn deployment(model: &str, protocol: ProtocolKind) -> Deployment {
    Deployment {
        id: format!("smoke/real/{model}").into(),
        endpoint_id: "smoke-ep".into(),
        protocol,
        model_id: model.into(),
    }
}

fn env_key(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn env_model(name: &str, fallback: &str) -> String {
    env_key(name).unwrap_or_else(|| fallback.to_string())
}

/// 无凭据时对官方端点的真实请求：必须得到 Canonical 认证错误而非传输失败。
fn assert_auth_error_without_key(
    adapter: &dyn ProviderAdapter,
    endpoint: &Endpoint,
    deployment: &Deployment,
) {
    let empty = InMemoryCredentialStore::new();
    let request = GenerateRequest::new(deployment.id.clone(), vec![Message::user("ping")]);
    let outcome = adapter.execute(
        &request,
        endpoint,
        deployment,
        &empty,
        &CredentialRef::from("smoke/unset"),
    );
    match outcome {
        Err(ModelError::AuthenticationFailed(d)) => {
            println!("  ↓ AuthenticationFailed: {}", d.message);
        }
        Err(ModelError::AuthorizationFailed(d)) => {
            println!("  ↓ AuthorizationFailed: {}", d.message);
        }
        Err(ModelError::ProviderError { status, detail }) => {
            // 少数端点对匿名请求返回 400/404；关键是"真实 HTTP 往返成功"
            println!("  ↓ ProviderError({status:?}): {}", detail.message);
        }
        Err(other) => panic!("expected a Canonical auth error, got {other:?}"),
        Ok(_) => println!("  ↓ 该端点匿名请求被接受（异常但非失败）"),
    }
}

#[test]
#[ignore = "requires real network"]
fn real_deepseek_reachable_and_errors_are_canonical() {
    println!("[smoke] deepseek 官方端点");
    let adapter = OpenAiChatAdapter::new(transport());
    assert_auth_error_without_key(
        &adapter,
        &endpoint("https://api.deepseek.com/v1", "deepseek"),
        &deployment("deepseek-chat", ProtocolKind::OpenAiChat),
    );

    // 发现端点同样可用（无凭据 → 认证错误，而非网络错误）
    let result = adapter.discover_models(
        &endpoint("https://api.deepseek.com/v1", "deepseek"),
        &InMemoryCredentialStore::new(),
        &CredentialRef::from("smoke/unset"),
    );
    println!("[smoke] discovery 结果: {result:?}");
    assert!(result.is_err(), "无凭据时 discovery 应失败");
}

#[test]
#[ignore = "requires real network"]
fn real_openai_reachable() {
    println!("[smoke] openai 官方端点（Chat）");
    let adapter = OpenAiChatAdapter::new(transport());
    assert_auth_error_without_key(
        &adapter,
        &endpoint("https://api.openai.com/v1", "openai"),
        &deployment("gpt-4o-mini", ProtocolKind::OpenAiChat),
    );

    println!("[smoke] openai 官方端点（Responses）");
    let adapter = OpenAiResponsesAdapter::new(transport());
    assert_auth_error_without_key(
        &adapter,
        &endpoint("https://api.openai.com/v1", "openai"),
        &deployment("gpt-4o-mini", ProtocolKind::OpenAiResponses),
    );
}

#[test]
#[ignore = "requires real network"]
fn real_anthropic_and_gemini_reachable() {
    println!("[smoke] anthropic 官方端点");
    let adapter = AnthropicAdapter::new(transport());
    assert_auth_error_without_key(
        &adapter,
        &endpoint("https://api.anthropic.com", "anthropic"),
        &deployment("claude-3-5-haiku-latest", ProtocolKind::AnthropicMessages),
    );

    println!("[smoke] gemini 官方端点");
    let adapter = GeminiAdapter::new(transport());
    assert_auth_error_without_key(
        &adapter,
        &endpoint("https://generativelanguage.googleapis.com", "google"),
        &deployment("gemini-2.5-flash", ProtocolKind::Gemini),
    );
}

/// 有凭据时的完整流式冒烟。
fn run_live(
    adapter: &dyn ProviderAdapter,
    endpoint: &Endpoint,
    deployment: &Deployment,
    credentials: &InMemoryCredentialStore,
) -> (Vec<SequencedEvent>, umber_engine::InvocationOutcome) {
    let request = GenerateRequest::new(
        deployment.id.clone(),
        vec![Message::user("用一句话说明你是什么模型。")],
    );
    let factory = {
        let endpoint = endpoint.clone();
        let deployment = deployment.clone();
        let request = request.clone();
        move || {
            adapter.execute(
                &request,
                &endpoint,
                &deployment,
                credentials,
                &CredentialRef::from(KEY_REF),
            )
        }
    };
    let mut events = Vec::new();
    let outcome = run_invocation(
        &factory,
        &request,
        &CancelToken::new(),
        &TimeoutPolicy::default(),
        &RetryPolicy::default(),
        &mut |e| events.push(e),
    );
    (events, outcome)
}

fn report(events: &[SequencedEvent], outcome: &umber_engine::InvocationOutcome, label: &str) {
    cassert::check_monotonic(events).unwrap_or_else(|e| panic!("{label}: 序列约束失败: {e}"));
    cassert::check_single_terminal(events)
        .unwrap_or_else(|e| panic!("{label}: 终结事件保证失败: {e}"));
    let text = outcome
        .response
        .as_ref()
        .map(|r| r.text_content())
        .unwrap_or_default();
    let stop = outcome.response.as_ref().map(|r| r.stop_reason);
    println!(
        "[smoke] {label}: {} 事件, stop_reason={stop:?}, usage={:?}",
        events.len(),
        outcome.response.as_ref().map(|r| r.usage.clone())
    );
    println!("[smoke] {label}: 回复 = {text}");
    assert!(!text.trim().is_empty(), "{label}: 真实调用必须返回非空文本");
}

#[test]
#[ignore = "requires real network + credentials"]
fn live_deepseek_streaming() {
    let Some(key) = env_key("UMER_SMOKE_DEEPSEEK_KEY") else {
        println!("[smoke] 跳过 deepseek 真实调用（未设置 UMER_SMOKE_DEEPSEEK_KEY）");
        return;
    };
    let adapter = OpenAiChatAdapter::new(transport());
    let credentials = store_with(&key);
    let deployment = deployment(
        &env_model("UMER_SMOKE_DEEPSEEK_MODEL", "deepseek-chat"),
        ProtocolKind::OpenAiChat,
    );
    let (events, outcome) = run_live(
        &adapter,
        &endpoint("https://api.deepseek.com/v1", "deepseek"),
        &deployment,
        &credentials,
    );
    report(&events, &outcome, "deepseek/openai_chat");

    // 真实发现的模型列表必须非空
    let models = adapter
        .discover_models(
            &endpoint("https://api.deepseek.com/v1", "deepseek"),
            &credentials,
            &CredentialRef::from(KEY_REF),
        )
        .expect("discovery 应成功");
    println!(
        "[smoke] deepseek 发现 {} 个模型，例如 {:?}",
        models.len(),
        models.first()
    );
    assert!(!models.is_empty());

    // 带工具调用的真实往返（§21 Tool 语义）
    let mut tool_request = GenerateRequest::new(
        deployment.id.clone(),
        vec![Message::user("杭州现在几点？用工具查。")],
    );
    tool_request.tools = vec![umber_core::request::Tool {
        name: "get_time".into(),
        description: "查询指定城市的当前时间".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }),
    }];
    tool_request.tool_choice = umber_core::request::ToolChoice::Required;
    let factory = {
        let endpoint = endpoint("https://api.deepseek.com/v1", "deepseek");
        let dep = deployment.clone();
        let creds = store_with(&key);
        let request = tool_request.clone();
        move || {
            adapter.execute(
                &request,
                &endpoint,
                &dep,
                &creds,
                &CredentialRef::from(KEY_REF),
            )
        }
    };
    let mut tool_events = Vec::new();
    let tool_outcome = run_invocation(
        &factory,
        &tool_request,
        &CancelToken::new(),
        &TimeoutPolicy::default(),
        &RetryPolicy::default(),
        &mut |e| tool_events.push(e),
    );
    cassert::check_single_terminal(&tool_events).unwrap();
    let calls: Vec<_> = tool_outcome
        .response
        .as_ref()
        .map(|r| r.tool_calls().collect::<Vec<_>>())
        .unwrap_or_default();
    println!("[smoke] deepseek 工具调用: {calls:?}");
    assert!(!calls.is_empty(), "tool_choice=required 时必须返回工具调用");
    assert_eq!(calls[0].name, "get_time");
}

#[test]
#[ignore = "requires real network + credentials"]
fn live_deepseek_cancellation_mid_stream() {
    let Some(key) = env_key("UMER_SMOKE_DEEPSEEK_KEY") else {
        println!("[smoke] 跳过取消测试（未设置 UMER_SMOKE_DEEPSEEK_KEY）");
        return;
    };
    let adapter = OpenAiChatAdapter::new(transport());
    let deployment = deployment(
        &env_model("UMER_SMOKE_DEEPSEEK_MODEL", "deepseek-chat"),
        ProtocolKind::OpenAiChat,
    );
    // 要一个长回复，确保有足够时间在中途取消
    let request = GenerateRequest::new(
        deployment.id.clone(),
        vec![Message::user("请详细写一篇 800 字关于海洋的科普文章。")],
    );
    let factory = {
        let endpoint = endpoint("https://api.deepseek.com/v1", "deepseek");
        let dep = deployment.clone();
        let creds = store_with(&key);
        let request = request.clone();
        move || {
            adapter.execute(
                &request,
                &endpoint,
                &dep,
                &creds,
                &CredentialRef::from(KEY_REF),
            )
        }
    };

    // 收到首个 TextDelta 就取消
    let cancel = CancelToken::new();
    let cancel_from_sink = cancel.clone();
    let mut events = Vec::new();
    let mut cancelled_on_first_delta = false;
    let outcome = run_invocation(
        &factory,
        &request,
        &cancel,
        &TimeoutPolicy::default(),
        &RetryPolicy::default(),
        &mut |event| {
            if !cancelled_on_first_delta
                && matches!(event.event, umber_core::event::ModelEvent::TextDelta { .. })
            {
                cancelled_on_first_delta = true;
                cancel_from_sink.cancel();
            }
            events.push(event);
        },
    );

    cassert::check_monotonic(&events).unwrap();
    cassert::check_single_terminal(&events).unwrap();
    let terminal = cassert::terminal(&events).unwrap();
    println!("[smoke] 真实取消：{} 个事件后终结", events.len());
    assert!(
        matches!(terminal.event, umber_core::event::ModelEvent::Cancelled),
        "取消必须产生 Cancelled 终结事件，实际为 {:?}",
        terminal.event
    );
    assert!(outcome.response.is_none(), "取消不应产生 Completed");
    // 部分结果必须可取回（§25.1）——真实网络下也成立
    let partial = outcome.partial.text();
    println!(
        "[smoke] 取消时已取回部分内容 {} 字",
        partial.chars().count()
    );
    assert!(
        !partial.trim().is_empty(),
        "取消时已产出的部分内容必须可取回"
    );
}

#[test]
#[ignore = "requires real network + credentials"]
fn live_openai_responses_streaming() {
    let Some(key) = env_key("UMER_SMOKE_OPENAI_KEY") else {
        println!("[smoke] 跳过 openai 真实调用（未设置 UMER_SMOKE_OPENAI_KEY）");
        return;
    };
    let adapter = OpenAiResponsesAdapter::new(transport());
    let credentials = store_with(&key);
    let deployment = deployment(
        &env_model("UMER_SMOKE_OPENAI_MODEL", "gpt-4o-mini"),
        ProtocolKind::OpenAiResponses,
    );
    let (events, outcome) = run_live(
        &adapter,
        &endpoint("https://api.openai.com/v1", "openai"),
        &deployment,
        &credentials,
    );
    report(&events, &outcome, "openai/openai_responses");
}

#[test]
#[ignore = "requires real network + credentials"]
fn live_anthropic_streaming() {
    let Some(key) = env_key("UMER_SMOKE_ANTHROPIC_KEY") else {
        println!("[smoke] 跳过 anthropic 真实调用（未设置 UMER_SMOKE_ANTHROPIC_KEY）");
        return;
    };
    let adapter = AnthropicAdapter::new(transport());
    let credentials = store_with(&key);
    let deployment = deployment(
        &env_model("UMER_SMOKE_ANTHROPIC_MODEL", "claude-3-5-haiku-latest"),
        ProtocolKind::AnthropicMessages,
    );
    let (events, outcome) = run_live(
        &adapter,
        &endpoint("https://api.anthropic.com", "anthropic"),
        &deployment,
        &credentials,
    );
    report(&events, &outcome, "anthropic/anthropic_messages");
}

#[test]
#[ignore = "requires real network + credentials"]
fn live_gemini_streaming() {
    let Some(key) = env_key("UMER_SMOKE_GEMINI_KEY") else {
        println!("[smoke] 跳过 gemini 真实调用（未设置 UMER_SMOKE_GEMINI_KEY）");
        return;
    };
    let adapter = GeminiAdapter::new(transport());
    let credentials = store_with(&key);
    let deployment = deployment(
        &env_model("UMER_SMOKE_GEMINI_MODEL", "gemini-2.5-flash"),
        ProtocolKind::Gemini,
    );
    let (events, outcome) = run_live(
        &adapter,
        &endpoint("https://generativelanguage.googleapis.com", "google"),
        &deployment,
        &credentials,
    );
    report(&events, &outcome, "google/gemini");
}

/// 传输层自身的可用性（不依赖任何 Provider）：真实 HTTPS GET。
#[test]
#[ignore = "requires real network"]
fn real_transport_round_trips_https() {
    use umber_protocol::HttpTransport;
    let t = RealHttpTransport::new(HttpConfig::default());
    let response = t
        .get("https://api.deepseek.com/v1/models", &vec![])
        .expect("TLS 往返必须成功");
    match response {
        umber_protocol::HttpResponse::Json { status, body } => {
            println!("[smoke] GET /v1/models -> {status} (JSON)");
            assert!(status == 401 || status == 403, "无凭据应为认证错误");
            assert!(body.get("error").is_some() || body.get("message").is_some());
        }
        umber_protocol::HttpResponse::Text { status, body } => {
            // 真实世界：DeepSeek 无凭据时返回纯文本 "Authentication Fails (governor)"
            println!("[smoke] GET /v1/models -> {status} (text): {body}");
            assert!(status == 401 || status == 403, "无凭据应为认证错误");
        }
        other => panic!("unexpected response shape: {other:?}"),
    }
    // 不存在的域名必须是 NetworkError（DNS 失败），而非 panic
    let err = t
        .get("https://no-such-host.invalid/v1/models", &vec![])
        .expect_err("DNS 失败应返回错误");
    assert!(matches!(err, ModelError::NetworkError(_)));
    println!("[smoke] DNS 失败正确映射为 NetworkError");
    // 保留 ScriptedTransport 的接口一致性断言
    let scripted = Arc::new(ScriptedTransport::new());
    let _: Arc<dyn HttpTransport> = scripted;
}
