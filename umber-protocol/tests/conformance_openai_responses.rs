//! M4 openai_responses conformance fixtures。
//!
//! 重点：encrypted_content 回传（§19.1）、function_call 全链路、
//! incomplete(max_output_tokens) → MaxTokens。

use std::sync::Arc;

use umber_conformance::assert as cassert;
use umber_core::content::ContentBlock;
use umber_core::error::ModelError;
use umber_core::message::Message;
use umber_core::request::{GenerateRequest, ReasoningConfig, ReasoningEffort};
use umber_core::response::StopReason;
use umber_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use umber_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use umber_model::deployment::{Deployment, Endpoint};
use umber_protocol::openai_responses::{build_request_body, OpenAiResponsesAdapter};
use umber_protocol::ScriptedTransport;
use umber_provider::ProviderAdapter;

const KEY_REF: &str = "test/key";

fn key() -> CredentialRef {
    CredentialRef::from(KEY_REF)
}

fn endpoint() -> Endpoint {
    Endpoint {
        id: "ep-1".into(),
        provider_id: "openai".into(),
        url: "https://api.openai.com/v1".into(),
    }
}

fn deployment() -> Deployment {
    Deployment {
        id: "openai/official/openai_responses/gpt-5".into(),
        endpoint_id: "ep-1".into(),
        protocol: umber_model::deployment::ProtocolKind::OpenAiResponses,
        model_id: "gpt-5".into(),
    }
}

fn credentials() -> InMemoryCredentialStore {
    let store = InMemoryCredentialStore::new();
    store.set(&key(), SecretString::new("sk-test")).unwrap();
    store
}

fn execute_through_engine(
    adapter: &OpenAiResponsesAdapter,
    request: &GenerateRequest,
) -> (
    Vec<umber_core::event::SequencedEvent>,
    umber_engine::InvocationOutcome,
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
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\"}}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"你好\"}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"，世界\"}\n\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":11,\"output_tokens\":4,\"input_tokens_details\":{\"cached_tokens\":6},\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n",
);

#[test]
fn fixture_plain_text_stream_completes() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, TEXT_SSE);
    let adapter = OpenAiResponsesAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("你好")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_monotonic(&events).unwrap();
    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(response.text_content(), "你好，世界");
    assert_eq!(response.usage.input_tokens, 11);
    assert_eq!(response.usage.cached_input_tokens, 6);
}

const REASONING_TOOL_SSE: &str = concat!(
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\"}}\n\n",
    "data: {\"type\":\"response.reasoning_summary_text.delta\",\"output_index\":0,\"delta\":\"查天气\"}\n\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"encrypted_content\":\"ENC-1\"}}\n\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call-x\",\"name\":\"get_weather\"}}\n\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"{\\\"city\\\":\\\"杭州\\\"}\"}\n\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call-x\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"reasoning\",\"summary\":[],\"encrypted_content\":\"ENC-1\"},{\"type\":\"function_call\",\"call_id\":\"call-x\",\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\\\"杭州\\\"}\"}],\"usage\":{\"input_tokens\":80,\"output_tokens\":30,\"output_tokens_details\":{\"reasoning_tokens\":20}}}}\n\n",
);

#[test]
fn fixture_reasoning_encrypted_content_roundtrips() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, REASONING_TOOL_SSE);
    let adapter = OpenAiResponsesAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("杭州天气")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    // 加密推理项进入 provider_payload（§19.1）
    let reasoning = response
        .content
        .iter()
        .find_map(|b| match b {
            ContentBlock::Reasoning(r) => Some(r),
            _ => None,
        })
        .expect("reasoning block");
    assert_eq!(
        reasoning.provider_payload.as_ref().unwrap()["encrypted_content"],
        "ENC-1".to_string()
    );
    let call = response.tool_calls().next().unwrap();
    assert_eq!(call.call_id.as_ref(), "call-x");
    assert_eq!(call.arguments_json, "{\"city\":\"杭州\"}");
}

#[test]
fn replay_sends_encrypted_reasoning_and_call_output() {
    // 多轮：宿主回放 encrypted reasoning + function_call/function_call_output
    let history = Message::assistant(vec![
        ContentBlock::Reasoning(umber_core::content::ReasoningBlock {
            text: String::new(),
            provider_payload: Some(serde_json::json!({"encrypted_content": "ENC-1"})),
        }),
        ContentBlock::ToolCall(umber_core::content::ToolCallBlock {
            call_id: "call-x".into(),
            name: "get_weather".into(),
            arguments_json: "{\"city\":\"杭州\"}".into(),
        }),
    ]);
    let tool_result = Message::new(
        umber_core::Role::Tool,
        vec![ContentBlock::ToolResult(
            umber_core::content::ToolResultBlock {
                call_id: "call-x".into(),
                is_error: false,
                content: vec![ContentBlock::text("晴 28°C")],
            },
        )],
    );
    let mut request = GenerateRequest::new(
        deployment().id,
        vec![Message::user("杭州天气"), history, tool_result],
    );
    request.reasoning = ReasoningConfig {
        effort: ReasoningEffort::High,
        exclude: false,
    };

    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    let input = body["input"].as_array().unwrap();
    // reasoning + function_call + function_call_output 依序回放
    assert_eq!(input[1]["type"], "reasoning");
    assert_eq!(input[1]["encrypted_content"], "ENC-1");
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(input[3]["type"], "function_call_output");
    assert_eq!(input[3]["call_id"], "call-x");
    // 推理档位（§21.1）
    assert_eq!(body["reasoning"]["effort"], "high");
    // 状态无关模式
    assert_eq!(body["store"], false);
}

const INCOMPLETE_SSE: &str = concat!(
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\"}}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"截断\"}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"截断\"}]}],\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}}\n\n",
);

#[test]
fn fixture_incomplete_max_tokens_maps_to_stop_reason() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, INCOMPLETE_SSE);
    let adapter = OpenAiResponsesAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::MaxTokens);
    assert_eq!(response.text_content(), "截断");
}

#[test]
fn fixture_http_429_maps_to_rate_limited() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        429,
        serde_json::json!({"error": {"message": "slow down", "code": "rate_limit_exceeded"}}),
    );
    let adapter = OpenAiResponsesAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let err = match adapter.execute(&request, &endpoint(), &deployment(), &credentials(), &key()) {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(err, ModelError::RateLimited { .. }));
}

#[test]
fn system_becomes_instructions_and_store_is_false() {
    let request = GenerateRequest::new(
        deployment().id,
        vec![Message::system("be brief"), Message::user("hi")],
    );
    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    assert_eq!(body["instructions"], "be brief");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
}
