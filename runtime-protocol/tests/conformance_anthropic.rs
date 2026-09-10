//! M3 anthropic_messages conformance fixtures。
//!
//! 重点：thinking 签名透传（§19.1）、缓存断点（§20）、529 过载映射。

use std::sync::Arc;

use runtime_conformance::assert as cassert;
use runtime_core::content::{CacheControl, ContentBlock};
use runtime_core::error::ModelError;
use runtime_core::event::ModelEvent;
use runtime_core::message::{Message, Role};
use runtime_core::request::{GenerateRequest, ReasoningConfig, ReasoningEffort, Tool, ToolChoice};
use runtime_core::response::StopReason;
use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use runtime_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use runtime_model::deployment::{Deployment, Endpoint};
use runtime_protocol::anthropic::{build_request_body, AnthropicAdapter};
use runtime_protocol::ScriptedTransport;
use runtime_provider::ProviderAdapter;

const KEY_REF: &str = "test/key";

fn key() -> CredentialRef {
    CredentialRef::from(KEY_REF)
}

fn endpoint() -> Endpoint {
    Endpoint {
        id: "ep-1".into(),
        provider_id: "anthropic".into(),
        url: "https://api.anthropic.com".into(),
    }
}

fn deployment() -> Deployment {
    Deployment {
        id: "anthropic/official/anthropic_messages/claude-sonnet".into(),
        endpoint_id: "ep-1".into(),
        protocol: runtime_model::deployment::ProtocolKind::AnthropicMessages,
        model_id: "claude-sonnet-4-5".into(),
    }
}

fn credentials() -> InMemoryCredentialStore {
    let store = InMemoryCredentialStore::new();
    store.set(&key(), SecretString::new("sk-ant")).unwrap();
    store
}

fn execute_through_engine(
    adapter: &AnthropicAdapter,
    request: &GenerateRequest,
) -> (
    Vec<runtime_core::event::SequencedEvent>,
    runtime_engine::InvocationOutcome,
) {
    let credentials = credentials();
    let endpoint = endpoint();
    let deployment = deployment();
    let request_for_adapter = request.clone();
    let factory = move || {
        adapter.execute(
            &request_for_adapter,
            &endpoint,
            &deployment,
            &credentials,
            &key(),
        )
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
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25}}}\n\n",
    "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你好\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"，世界\"}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":8}}\n\n",
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
);

#[test]
fn fixture_plain_text_stream_completes() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, TEXT_SSE);
    let adapter = AnthropicAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("你好")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_monotonic(&events).unwrap();
    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(response.text_content(), "你好，世界");
    assert_eq!(response.usage.input_tokens, 25);
    assert_eq!(response.usage.output_tokens, 8);
}

const THINKING_TOOL_SSE: &str = concat!(
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100}}}\n\n",
    "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"需要查天气\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-ABC\"}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-1\",\"name\":\"get_weather\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"杭州\\\"}\"}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
    "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":50}}\n\n",
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
);

#[test]
fn fixture_thinking_signature_flows_to_provider_payload() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, THINKING_TOOL_SSE);
    let adapter = AnthropicAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("杭州天气")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    assert!(events.iter().any(
        |e| matches!(e.event, ModelEvent::ReasoningDelta { ref delta, .. } if delta == "需要查天气")
    ));
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    // thinking 签名进入 provider_payload，供多轮原样回传（§19.1）
    let reasoning = response.content.iter().find_map(|b| match b {
        ContentBlock::Reasoning(r) => Some(r),
        _ => None,
    });
    let r = reasoning.expect("reasoning block present");
    assert_eq!(r.text, "需要查天气");
    assert_eq!(
        r.provider_payload.as_ref().unwrap()["signature"],
        "sig-ABC".to_string()
    );
    let call = response.tool_calls().next().expect("tool call present");
    assert_eq!(call.call_id.as_ref(), "toolu-1");
    assert_eq!(call.arguments_json, "{\"city\":\"杭州\"}");
}

#[test]
fn replay_includes_thinking_block_with_signature() {
    // 多轮：宿主回放带 provider_payload 的 Reasoning 块 → 请求体必须
    // 含 {"type":"thinking","thinking":…,"signature":…}
    let history_message = Message::assistant(vec![
        ContentBlock::Reasoning(runtime_core::content::ReasoningBlock {
            text: "需要查天气".into(),
            provider_payload: Some(serde_json::json!({"signature": "sig-ABC"})),
        }),
        ContentBlock::ToolCall(runtime_core::content::ToolCallBlock {
            call_id: "toolu-1".into(),
            name: "get_weather".into(),
            arguments_json: "{\"city\":\"杭州\"}".into(),
        }),
    ]);
    let tool_result = Message::new(
        Role::Tool,
        vec![ContentBlock::ToolResult(
            runtime_core::content::ToolResultBlock {
                call_id: "toolu-1".into(),
                is_error: false,
                content: vec![ContentBlock::text("晴 28°C")],
            },
        )],
    );
    let mut request = GenerateRequest::new(
        deployment().id,
        vec![Message::user("杭州天气"), history_message, tool_result],
    );
    request.reasoning = ReasoningConfig {
        effort: ReasoningEffort::High,
        exclude: false,
    };

    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    let msgs = body["messages"].as_array().unwrap();
    // assistant 消息：thinking + tool_use
    let assistant = &msgs[1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"][0]["type"], "thinking");
    assert_eq!(assistant["content"][0]["signature"], "sig-ABC");
    assert_eq!(assistant["content"][1]["type"], "tool_use");
    // tool 结果：user 消息内的 tool_result
    let tool_msg = &msgs[2];
    assert_eq!(tool_msg["role"], "user");
    assert_eq!(tool_msg["content"][0]["type"], "tool_result");
    // thinking 档位映射（§21.1）
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(
        body["thinking"]["budget_tokens"],
        runtime_protocol::anthropic::effort_to_budget(ReasoningEffort::High)
    );
}

#[test]
fn cache_control_marks_system_blocks() {
    let mut system = Message::system("long instructions");
    if let ContentBlock::Text(t) = &mut system.content[0] {
        t.cache_control = Some(CacheControl::Ephemeral);
    }
    let request = GenerateRequest::new(deployment().id, vec![system, Message::user("hi")]);
    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn fixture_overloaded_529_maps_to_overloaded() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        529,
        serde_json::json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
    );
    let adapter = AnthropicAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let err = match adapter.execute(&request, &endpoint(), &deployment(), &credentials(), &key()) {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, ModelError::Overloaded(_)));
    assert!(err.retryable());
}

#[test]
fn fixture_prompt_too_long_maps_to_context_exceeded() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        400,
        serde_json::json!({"type": "error", "error": {"type": "invalid_request_error", "message": "prompt is too long: 200000 tokens > 100000 maximum"}}),
    );
    let adapter = AnthropicAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let err = match adapter.execute(&request, &endpoint(), &deployment(), &credentials(), &key()) {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, ModelError::ContextExceeded(_)));
}

#[test]
fn request_body_carries_anthropic_essentials() {
    let mut request = GenerateRequest::new(
        deployment().id,
        vec![Message::system("sys"), Message::user("hi")],
    );
    request.tools = vec![Tool {
        name: "f".into(),
        description: "d".into(),
        input_schema: serde_json::json!({"type": "object"}),
    }];
    request.tool_choice = ToolChoice::Required;

    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    assert_eq!(body["model"], "claude-sonnet-4-5");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["system"][0]["type"], "text");
    assert_eq!(body["tools"][0]["name"], "f");
    assert_eq!(body["tool_choice"]["type"], "any");
    assert_eq!(body["stream"], true);
    // 默认档位不发 thinking
    assert!(body.get("thinking").is_none());
}

#[test]
fn fixture_discovery_lists_models() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        200,
        serde_json::json!({"data": [{"id": "claude-opus-4-1", "display_name": "Claude Opus 4.1"}]}),
    );
    let adapter = AnthropicAdapter::new(transport);
    let models = adapter
        .discover_models(&endpoint(), &credentials(), &key())
        .unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].display_name.as_deref(), Some("Claude Opus 4.1"));
}
