//! M2 openai_chat conformance fixtures（总案 §52 最低覆盖的 openai_chat 部分）。
//!
//! 每条 fixture 走完整链路：ScriptedTransport → Adapter → Engine → 断言。

use std::sync::Arc;

use umber_conformance::assert as cassert;
use umber_core::content::ContentBlock;
use umber_core::error::ModelError;
use umber_core::event::ModelEvent;
use umber_core::message::Message;
use umber_core::request::{GenerateRequest, ReasoningEffort, ResponseFormat, Tool, ToolChoice};
use umber_core::response::StopReason;
use umber_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use umber_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use umber_model::deployment::{Deployment, Endpoint};
use umber_protocol::openai_chat::{build_request_body, OpenAiChatAdapter};
use umber_protocol::ScriptedTransport;
use umber_provider::ProviderAdapter;

const KEY_REF: &str = "test/key";

fn endpoint() -> Endpoint {
    Endpoint {
        id: "ep-1".into(),
        provider_id: "deepseek".into(),
        url: "https://api.deepseek.com/v1".into(),
    }
}

fn deployment() -> Deployment {
    Deployment {
        id: "deepseek/official/openai_chat/deepseek-chat".into(),
        endpoint_id: "ep-1".into(),
        protocol: umber_model::deployment::ProtocolKind::OpenAiChat,
        model_id: "deepseek-chat".into(),
    }
}

fn credentials() -> InMemoryCredentialStore {
    let store = InMemoryCredentialStore::new();
    store.set(&key(), SecretString::new("sk-test")).unwrap();
    store
}

fn key() -> CredentialRef {
    CredentialRef::from(KEY_REF)
}

/// execute 的错误断言助手（Box<dyn ProviderStream> 无 Debug，不能用 unwrap_err）。
fn execute_err(adapter: &OpenAiChatAdapter, request: &GenerateRequest) -> ModelError {
    match adapter.execute(request, &endpoint(), &deployment(), &credentials(), &key()) {
        Err(e) => e,
        Ok(_) => panic!("expected execute to fail"),
    }
}

fn adapter_with_sse(sse: &str) -> (OpenAiChatAdapter, Arc<ScriptedTransport>) {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, sse);
    let adapter = OpenAiChatAdapter::new(transport.clone());
    (adapter, transport)
}

fn execute_through_engine(
    adapter: &OpenAiChatAdapter,
    request: &GenerateRequest,
) -> (
    Vec<umber_core::event::SequencedEvent>,
    umber_engine::InvocationOutcome,
) {
    let credentials = credentials();
    let endpoint = endpoint();
    let deployment = deployment();
    let factory = {
        let endpoint = endpoint.clone();
        let deployment = deployment.clone();
        let request = request.clone();
        move || adapter.execute(&request, &endpoint, &deployment, &credentials, &key())
    };
    let mut events = Vec::new();
    let outcome = run_invocation(
        &factory,
        request,
        &CancelToken::new(),
        &TimeoutPolicy::default(),
        &RetryPolicy::default(),
        &mut |e| events.push(e),
    );
    (events, outcome)
}

const TEXT_SSE: &str = concat!(
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"你\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"好，世界\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":3,\"total_tokens\":13,\"prompt_tokens_details\":{\"cached_tokens\":4}}}\n\n",
    "data: [DONE]\n\n",
);

#[test]
fn fixture_plain_text_stream_completes() {
    let (adapter, _) = adapter_with_sse(TEXT_SSE);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("你好")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_monotonic(&events).unwrap();
    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(response.text_content(), "你好，世界");
    assert_eq!(response.usage.input_tokens, 10);
    assert_eq!(response.usage.cached_input_tokens, 4);
    // Completed 的 invocation_id 被 Engine 覆写
    assert_eq!(response.invocation_id, outcome.invocation_id);
}

const TOOL_SSE: &str = concat!(
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-a\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\\\"杭\"}},{\"index\":1,\"id\":\"call-b\",\"type\":\"function\",\"function\":{\"name\":\"get_time\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"州\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: [DONE]\n\n",
);

#[test]
fn fixture_parallel_tool_calls_reassemble() {
    let (adapter, _) = adapter_with_sse(TOOL_SSE);
    let mut request = GenerateRequest::new(deployment().id, vec![Message::user("天气和时间")]);
    request.tools = vec![Tool {
        name: "get_weather".into(),
        description: "天气".into(),
        input_schema: serde_json::json!({"type": "object"}),
    }];
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    let calls: Vec<_> = response.tool_calls().collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].call_id.as_ref(), "call-a");
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].arguments_json, "{\"city\":\"杭州\"}");
    assert_eq!(calls[1].call_id.as_ref(), "call-b");
    assert_eq!(calls[1].arguments_json, "{}");
}

const REASONING_SSE: &str = concat!(
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"先想\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"再答\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"答案\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: [DONE]\n\n",
);

#[test]
fn fixture_reasoning_content_maps_to_reasoning_events() {
    let (adapter, _) = adapter_with_sse(REASONING_SSE);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("问题")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    assert!(events.iter().any(
        |e| matches!(e.event, ModelEvent::ReasoningDelta { ref delta, .. } if delta == "先想")
    ));
    let response = outcome.response.expect("must complete");
    assert!(response
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Reasoning(r) if r.text == "先想再答")));
    assert_eq!(response.text_content(), "答案");
}

#[test]
fn fixture_http_429_maps_to_rate_limited() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        429,
        serde_json::json!({"error": {"message": "rate", "type": "requests", "code": "rate_limit_exceeded"}}),
    );
    let adapter = OpenAiChatAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let err = execute_err(&adapter, &request);
    assert!(matches!(err, ModelError::RateLimited { .. }));
    assert!(err.retryable());
}

#[test]
fn fixture_quota_error_refines_to_authorization() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        429,
        serde_json::json!({"error": {"message": "quota", "code": "insufficient_quota"}}),
    );
    let adapter = OpenAiChatAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let err = execute_err(&adapter, &request);
    assert!(matches!(err, ModelError::AuthorizationFailed(_)));
    assert!(!err.retryable());
}

#[test]
fn fixture_context_length_message_maps_to_context_exceeded() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        400,
        serde_json::json!({"error": {"message": "This model's maximum context length is 4096 tokens", "code": "400"}}),
    );
    let adapter = OpenAiChatAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let err = execute_err(&adapter, &request);
    assert!(matches!(err, ModelError::ContextExceeded(_)));
}

#[test]
fn fixture_discovery_lists_models() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        200,
        serde_json::json!({"data": [{"id": "deepseek-chat"}, {"id": "deepseek-reasoner"}]}),
    );
    let adapter = OpenAiChatAdapter::new(transport);
    let models = adapter
        .discover_models(&endpoint(), &credentials(), &key())
        .unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].model_id, "deepseek-chat");
}

#[test]
fn request_body_matches_openai_chat_shape() {
    let mut request = GenerateRequest::new(
        "dep",
        vec![Message::system("be brief"), Message::user("hi")],
    );
    request.tools = vec![Tool {
        name: "f".into(),
        description: "d".into(),
        input_schema: serde_json::json!({"type": "object"}),
    }];
    request.tool_choice = ToolChoice::Required;
    request.reasoning.effort = ReasoningEffort::High;
    request.generation.temperature = Some(0.5);
    request.response_format = ResponseFormat::JsonSchema {
        name: "out".into(),
        schema: serde_json::json!({"type": "object"}),
        strict: true,
    };

    let body = build_request_body(&request, "deepseek-chat").unwrap();
    assert_eq!(body["model"], "deepseek-chat");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["tool_choice"], "required");
    assert_eq!(body["reasoning_effort"], "high");
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert!(body["tools"].as_array().unwrap()[0]["function"]["name"] == "f");
}

#[test]
fn request_body_omits_default_reasoning_effort() {
    let request = GenerateRequest::new("dep", vec![Message::user("hi")]);
    let body = build_request_body(&request, "m").unwrap();
    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["messages"][0]["content"], "hi");
}
