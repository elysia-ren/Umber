//! M5 gemini conformance fixtures。
//!
//! 重点：safety 拦截 → ContentFiltered、finishReason 映射、
//! functionCall 完整下发、responseSchema 子集映射、thinkingBudget。

use std::sync::Arc;

use umber_conformance::assert as cassert;
use umber_core::error::ModelError;
use umber_core::message::Message;
use umber_core::request::{GenerateRequest, ReasoningConfig, ReasoningEffort, ResponseFormat};
use umber_core::response::StopReason;
use umber_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use umber_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use umber_model::deployment::{Deployment, Endpoint};
use umber_protocol::gemini::{
    build_request_body, effort_to_thinking_budget, to_gemini_schema, GeminiAdapter,
};
use umber_protocol::ScriptedTransport;
use umber_provider::ProviderAdapter;

const KEY_REF: &str = "test/key";

fn key() -> CredentialRef {
    CredentialRef::from(KEY_REF)
}

fn endpoint() -> Endpoint {
    Endpoint {
        id: "ep-1".into(),
        provider_id: "google".into(),
        url: "https://generativelanguage.googleapis.com".into(),
    }
}

fn deployment() -> Deployment {
    Deployment {
        id: "google/official/gemini/gemini-2.5-flash".into(),
        endpoint_id: "ep-1".into(),
        protocol: umber_model::deployment::ProtocolKind::Gemini,
        model_id: "gemini-2.5-flash".into(),
    }
}

fn credentials() -> InMemoryCredentialStore {
    let store = InMemoryCredentialStore::new();
    store.set(&key(), SecretString::new("g-key")).unwrap();
    store
}

fn execute_through_engine(
    adapter: &GeminiAdapter,
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
    "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"你好\"}],\"role\":\"model\"},\"index\":0}]}\n\n",
    "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"，世界\"}],\"role\":\"model\"},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"promptTokenCount\":9,\"candidatesTokenCount\":4,\"totalTokenCount\":13}}\n\n",
);

#[test]
fn fixture_plain_text_stream_completes_at_eof() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, TEXT_SSE);
    let adapter = GeminiAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("你好")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_monotonic(&events).unwrap();
    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(response.text_content(), "你好，世界");
    assert_eq!(response.usage.input_tokens, 9);
    assert_eq!(response.usage.output_tokens, 4);
}

const FUNCTION_CALL_SSE: &str = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"杭州\"}}}],\"role\":\"model\"},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"promptTokenCount\":20,\"candidatesTokenCount\":10,\"thoughtsTokenCount\":5,\"totalTokenCount\":35}}\n\n";

#[test]
fn fixture_function_call_completes_with_tool_use() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, FUNCTION_CALL_SSE);
    let adapter = GeminiAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("杭州天气")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    let call = response.tool_calls().next().unwrap();
    assert_eq!(call.name, "get_weather");
    assert_eq!(call.arguments_json, "{\"city\":\"杭州\"}");
    assert_eq!(response.usage.reasoning_tokens, 5);
}

const SAFETY_SSE: &str = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"部分\"}],\"role\":\"model\"},\"finishReason\":\"SAFETY\",\"index\":0}]}\n\n";

#[test]
fn fixture_safety_finish_reason_maps_to_safety_blocked() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, SAFETY_SSE);
    let adapter = GeminiAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::SafetyBlocked);
    // 部分输出必须可取回（§25.1）
    assert_eq!(outcome.partial.text(), "部分");
}

#[test]
fn fixture_prompt_blocked_maps_to_content_filtered() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        200,
        serde_json::json!({"promptFeedback": {"blockReason": "SAFETY"}}),
    );
    let adapter = GeminiAdapter::new(transport);
    let request = GenerateRequest::new(deployment().id, vec![Message::user("hi")]);
    let (events, outcome) = execute_through_engine(&adapter, &request);

    cassert::check_single_terminal(&events).unwrap();
    let terminal = cassert::terminal(&events).unwrap();
    match &terminal.event {
        umber_core::event::ModelEvent::Failed { error } => {
            assert!(matches!(error, ModelError::ContentFiltered(_)));
            assert!(!error.retryable());
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(outcome.response.is_none());
}

#[test]
fn fixture_discovery_filters_non_generate_models() {
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_json(
        200,
        serde_json::json!({"models": [
            {"name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash",
             "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/text-embedding", "supportedGenerationMethods": ["embedContent"]}
        ]}),
    );
    let adapter = GeminiAdapter::new(transport);
    let models = adapter
        .discover_models(&endpoint(), &credentials(), &key())
        .unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model_id, "gemini-2.5-flash");
    assert_eq!(models[0].display_name.as_deref(), Some("Gemini 2.5 Flash"));
}

#[test]
fn request_body_carries_gemini_essentials() {
    let mut request = GenerateRequest::new(
        deployment().id,
        vec![Message::system("sys"), Message::user("hi")],
    );
    request.response_format = ResponseFormat::JsonSchema {
        name: "out".into(),
        schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {"city": {"type": "string", "enum": ["杭州"]}}
        }),
        strict: true,
    };
    request.reasoning = ReasoningConfig {
        effort: ReasoningEffort::Low,
        exclude: false,
    };

    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "sys");
    assert_eq!(body["contents"][0]["role"], "user");
    // responseSchema 子集映射：type 大写、additionalProperties 丢弃（§18.1）
    let schema = &body["generationConfig"]["responseSchema"];
    assert_eq!(schema["type"], "OBJECT");
    assert!(schema.get("additionalProperties").is_none());
    assert_eq!(schema["properties"]["city"]["type"], "STRING");
    assert_eq!(schema["properties"]["city"]["enum"][0], "杭州");
    // thinkingBudget 映射（§21.1）
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        effort_to_thinking_budget(ReasoningEffort::Low)
    );
}

#[test]
fn schema_mapper_is_recursive() {
    let out = to_gemini_schema(&serde_json::json!({
        "type": "array",
        "items": {"type": "object", "properties": {"n": {"type": "integer"}}}
    }));
    assert_eq!(out["type"], "ARRAY");
    assert_eq!(out["items"]["type"], "OBJECT");
    assert_eq!(out["items"]["properties"]["n"]["type"], "INTEGER");
}

#[test]
fn tool_result_uses_function_response_with_name() {
    let history = Message::assistant(vec![umber_core::content::ContentBlock::ToolCall(
        umber_core::content::ToolCallBlock {
            call_id: "call-1".into(),
            name: "get_weather".into(),
            arguments_json: "{\"city\":\"杭州\"}".into(),
        },
    )]);
    let tool_result = Message::new(
        umber_core::Role::Tool,
        vec![umber_core::content::ContentBlock::ToolResult(
            umber_core::content::ToolResultBlock {
                call_id: "call-1".into(),
                is_error: false,
                content: vec![umber_core::content::ContentBlock::text("晴 28°C")],
            },
        )],
    );
    let request = GenerateRequest::new(
        deployment().id,
        vec![Message::user("杭州天气"), history, tool_result],
    );
    let body = build_request_body(&request, deployment().model_id.as_str()).unwrap();
    let contents = body["contents"].as_array().unwrap();
    // model 角色 + functionCall
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(
        contents[1]["parts"][0]["functionCall"]["name"],
        "get_weather"
    );
    // user 角色 + functionResponse（按函数名配对）
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["result"],
        "晴 28°C"
    );
}
