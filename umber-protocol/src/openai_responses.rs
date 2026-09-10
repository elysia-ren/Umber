//! OpenAI Responses 协议 Adapter（总案 §4，M4）。
//!
//! 请求映射要点：
//! - system → `instructions`；对话历史以 `input[]` 项承载
//! - Reasoning.provider_payload 存放 `encrypted_content`；多轮回放时重组为
//!   `{"type":"reasoning","encrypted_content":…}` 原样回传（总案 §19.1——
//!   丢掉它，推理模型的多轮工具调用会被拒）
//! - ToolResult → `function_call_output`；历史 ToolCall → `function_call`
//! - 结构化输出 → `text.format`（native strict）
//!
//! 流解析要点：SSE 数据自带 `type` 字段（与 event: 名一致）；
//! 终态以 `response.completed` / `response.failed` 为准。

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use umber_core::content::ContentBlock;
use umber_core::error::{ErrorDetail, ModelError};
use umber_core::event::ModelEvent;
use umber_core::ids::{BlockId, CallId};
use umber_core::message::Role;
use umber_core::request::{GenerateRequest, ReasoningEffort, ResponseFormat, ToolChoice};
use umber_core::response::{GenerateResponse, ProviderContext, StopReason};
use umber_core::usage::Usage;
use umber_credential::{CredentialRef, CredentialStore};
use umber_engine::{CancelToken, ProviderStream, StreamAssembler};
use umber_model::deployment::{Deployment, Endpoint, ProtocolKind};
use umber_provider::{AdapterDescriptor, DiscoveredModel, ProviderAdapter};

use crate::errors::{extract_code_message, map_status, refine};
use crate::join_url;
use crate::jsonh;
use crate::sse::SseAssembler;
use crate::transport::{Headers, HttpResponse, HttpTransport};
use crate::worker::ChannelStream;

pub struct OpenAiResponsesAdapter {
    transport: Arc<dyn HttpTransport>,
}

impl OpenAiResponsesAdapter {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }

    pub fn responses_url(endpoint: &Endpoint) -> String {
        join_url(&endpoint.url, "responses")
    }

    fn headers(credentials: &dyn CredentialStore, credential_ref: &CredentialRef) -> Headers {
        let mut headers: Headers = vec![
            ("content-type".into(), "application/json".into()),
            (
                "accept".into(),
                "application/json, text/event-stream".into(),
            ),
        ];
        if let Ok(Some(secret)) = credentials.get(credential_ref) {
            if !secret.expose().is_empty() {
                headers.push((
                    "authorization".into(),
                    format!("Bearer {}", secret.expose()),
                ));
            }
        }
        headers
    }

    fn map_error(status: u16, body: &serde_json::Value) -> ModelError {
        let (code, message) = extract_code_message(body);
        let base = map_status(status, code.clone(), message.clone());
        refine(base, code.as_deref(), &message)
    }
}

impl ProviderAdapter for OpenAiResponsesAdapter {
    fn describe(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            provider_id: "openai_responses".into(),
            display_name: "OpenAI Responses".into(),
            protocols: vec![ProtocolKind::OpenAiResponses],
        }
    }

    fn discover_models(
        &self,
        endpoint: &Endpoint,
        credentials: &dyn CredentialStore,
        credential_ref: &CredentialRef,
    ) -> Result<Vec<DiscoveredModel>, ModelError> {
        // 与 Chat 共享 /models 列表端点
        let url = join_url(&endpoint.url, "models");
        match self
            .transport
            .get(&url, &Self::headers(credentials, credential_ref))?
        {
            HttpResponse::Json { status, body } if (200..300).contains(&status) => {
                let mut models = Vec::new();
                if let Some(items) = jsonh::as_array(&body, "data") {
                    for item in items {
                        if let Some(id) = jsonh::as_str(item, "id") {
                            models.push(DiscoveredModel {
                                model_id: id.to_string(),
                                display_name: jsonh::as_str(item, "display_name")
                                    .map(str::to_string),
                            });
                        }
                    }
                }
                Ok(models)
            }
            HttpResponse::Json { status, body } => Err(Self::map_error(status, &body)),
            HttpResponse::Text { status, body } => Err(map_status(status, None, body)),
            HttpResponse::Sse { .. } => Err(ModelError::InvalidRequest(ErrorDetail::new(
                "discovery returned an event stream",
            ))),
        }
    }

    fn execute(
        &self,
        request: &GenerateRequest,
        endpoint: &Endpoint,
        deployment: &Deployment,
        credentials: &dyn CredentialStore,
        credential_ref: &CredentialRef,
    ) -> Result<Box<dyn ProviderStream>, ModelError> {
        let body = build_request_body(request, &deployment.model_id)?;
        let url = Self::responses_url(endpoint);
        match self.transport.post_stream(
            &url,
            &Self::headers(credentials, credential_ref),
            body.to_string(),
        )? {
            HttpResponse::Sse { lines } => {
                let cancel = CancelToken::new();
                let mut sse = SseAssembler::new();
                let mut parser = ResponsesStreamParser::new();
                Ok(Box::new(ChannelStream::spawn(
                    cancel,
                    lines,
                    move |line, out| {
                        if let Some(event) = sse.feed_line(line) {
                            parser.line(&event.data, out);
                        }
                        true
                    },
                )))
            }
            HttpResponse::Json { status, body } => Err(Self::map_error(status, &body)),
            HttpResponse::Text { status, body } => Err(map_status(status, None, body)),
        }
    }
}

/// 请求体构造（纯函数，fixtures 直接断言）。
pub fn build_request_body(
    request: &GenerateRequest,
    model_id: &str,
) -> Result<serde_json::Value, ModelError> {
    request
        .validate()
        .map_err(|e| ModelError::InvalidRequest(ErrorDetail::new(e.to_string())))?;

    let mut instructions = String::new();
    let mut input: Vec<serde_json::Value> = Vec::new();

    for message in &request.messages {
        match message.role {
            Role::System => {
                for b in &message.content {
                    if let Some(t) = b.as_text() {
                        if !instructions.is_empty() {
                            instructions.push('\n');
                        }
                        instructions.push_str(t);
                    }
                }
            }
            Role::User => {
                let content: Vec<serde_json::Value> = message
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(t) => {
                            Some(json!({"type": "input_text", "text": t.text}))
                        }
                        ContentBlock::Image(i) => match &i.source {
                            umber_core::content::MediaSource::Url { url } => {
                                Some(json!({"type": "input_image", "image_url": url}))
                            }
                            umber_core::content::MediaSource::Base64 { media_type, data } => {
                                Some(json!({
                                    "type": "input_image",
                                    "image_url": format!("data:{media_type};base64,{data}")
                                }))
                            }
                        },
                        _ => None,
                    })
                    .collect();
                input.push(json!({"role": "user", "content": content}));
            }
            Role::Assistant => {
                for b in &message.content {
                    match b {
                        ContentBlock::Text(t) if !t.text.is_empty() => {
                            input.push(json!({
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": t.text}]
                            }));
                        }
                        // 加密推理项原样回传；丢掉它，推理模型的多轮工具调用会被拒（§19.1）
                        ContentBlock::Reasoning(r) => {
                            if let Some(encrypted) = r
                                .provider_payload
                                .as_ref()
                                .and_then(|p| p.get("encrypted_content"))
                                .and_then(|e| e.as_str())
                            {
                                input.push(json!({
                                    "type": "reasoning",
                                    "encrypted_content": encrypted
                                }));
                            }
                        }
                        ContentBlock::ToolCall(c) => {
                            input.push(json!({
                                "type": "function_call",
                                "call_id": c.call_id.as_ref(),
                                "name": c.name,
                                "arguments": c.arguments_json
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Role::Tool => {
                for b in &message.content {
                    if let ContentBlock::ToolResult(r) = b {
                        let output = r
                            .content
                            .iter()
                            .filter_map(|b| b.as_text())
                            .collect::<Vec<_>>()
                            .join("\n");
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": r.call_id.as_ref(),
                            "output": output
                        }));
                    }
                }
            }
        }
    }

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model_id));
    body.insert("input".into(), json!(input));
    if !instructions.is_empty() {
        body.insert("instructions".into(), json!(instructions));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            json!(request
                .tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                    "strict": false
                }))
                .collect::<Vec<_>>()),
        );
    }
    match &request.tool_choice {
        ToolChoice::Auto => {}
        ToolChoice::None => {
            body.insert("tool_choice".into(), json!("none"));
        }
        ToolChoice::Required => {
            body.insert("tool_choice".into(), json!("required"));
        }
        ToolChoice::Specific { name } => {
            body.insert(
                "tool_choice".into(),
                json!({"type": "function", "name": name}),
            );
        }
    }
    if request.reasoning.effort != ReasoningEffort::Medium {
        let value = match request.reasoning.effort {
            ReasoningEffort::Minimal => "minimal",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        };
        body.insert("reasoning".into(), json!({"effort": value}));
    }
    let g = &request.generation;
    if let Some(v) = g.max_output_tokens {
        body.insert("max_output_tokens".into(), json!(v));
    }
    if let Some(v) = g.temperature {
        body.insert("temperature".into(), json!(v));
    }
    if let Some(v) = g.top_p {
        body.insert("top_p".into(), json!(v));
    }
    if let Some(seed) = g.seed {
        body.insert("seed".into(), json!(seed));
    }
    match &request.response_format {
        ResponseFormat::Text => {}
        ResponseFormat::JsonObject => {
            body.insert("text".into(), json!({"format": {"type": "json_object"}}));
        }
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => {
            body.insert(
                "text".into(),
                json!({"format": {
                    "type": "json_schema",
                    "name": name,
                    "schema": schema,
                    "strict": strict
                }}),
            );
        }
    }
    // Runtime 不依赖服务端会话状态（总案 §61：Conversation state 暂缓）
    body.insert("store".into(), json!(false));
    body.insert("stream".into(), json!(true));

    Ok(serde_json::Value::Object(body))
}

/// 流解析状态机（按数据 JSON 的 `type` 字段分派）。
struct ResponsesStreamParser {
    assembler: StreamAssembler,
    /// output_index → (kind, block_id / call_id)
    open_items: BTreeMap<u64, ItemRef>,
    seen_content: bool,
}

#[derive(Clone)]
enum ItemRef {
    Message(BlockId),
    Reasoning(BlockId),
    FunctionCall(CallId),
}

impl ResponsesStreamParser {
    fn new() -> Self {
        Self {
            assembler: StreamAssembler::new(),
            open_items: BTreeMap::new(),
            seen_content: false,
        }
    }

    fn emit(&mut self, out: &mut Vec<ModelEvent>, event: ModelEvent) {
        self.assembler.absorb(&event);
        out.push(event);
    }

    fn line(&mut self, data: &str, out: &mut Vec<ModelEvent>) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
            return;
        };
        let Some(kind) = jsonh::as_str(&v, "type") else {
            return;
        };
        match kind {
            "response.output_item.added" => {
                let index = jsonh::as_u64(&v, "output_index").unwrap_or(0);
                let item_type = jsonh::at_str(&v, &["item", "type"]).unwrap_or("");
                match item_type {
                    "message" => {
                        let block_id = BlockId::from(format!("o{index}"));
                        self.open_items
                            .insert(index, ItemRef::Message(block_id.clone()));
                        self.emit(out, ModelEvent::TextStarted { block_id });
                    }
                    "reasoning" => {
                        let block_id = BlockId::from(format!("o{index}"));
                        self.open_items
                            .insert(index, ItemRef::Reasoning(block_id.clone()));
                        self.emit(out, ModelEvent::ReasoningStarted { block_id });
                    }
                    "function_call" => {
                        let call_id = jsonh::at_str(&v, &["item", "call_id"])
                            .map(CallId::from)
                            .unwrap_or_else(|| CallId::from(format!("call-{index}")));
                        let name = jsonh::at_str(&v, &["item", "name"]).unwrap_or_default();
                        self.open_items
                            .insert(index, ItemRef::FunctionCall(call_id.clone()));
                        self.emit(
                            out,
                            ModelEvent::ToolCallStarted {
                                call_id,
                                name: name.to_string(),
                            },
                        );
                    }
                    _ => {}
                }
            }
            "response.output_text.delta" => {
                let index = jsonh::as_u64(&v, "output_index").unwrap_or(0);
                if let Some(delta) = jsonh::as_str(&v, "delta") {
                    self.seen_content = true;
                    self.emit(
                        out,
                        ModelEvent::TextDelta {
                            block_id: BlockId::from(format!("o{index}")),
                            delta: delta.to_string(),
                        },
                    );
                }
            }
            "response.reasoning_summary_text.delta" => {
                let index = jsonh::as_u64(&v, "output_index").unwrap_or(0);
                if let Some(delta) = jsonh::as_str(&v, "delta") {
                    self.seen_content = true;
                    self.emit(
                        out,
                        ModelEvent::ReasoningDelta {
                            block_id: BlockId::from(format!("o{index}")),
                            delta: delta.to_string(),
                        },
                    );
                }
            }
            "response.function_call_arguments.delta" => {
                let index = jsonh::as_u64(&v, "output_index").unwrap_or(0);
                if let Some(ItemRef::FunctionCall(call_id)) = self.open_items.get(&index) {
                    if let Some(delta) = jsonh::as_str(&v, "delta") {
                        self.emit(
                            out,
                            ModelEvent::ToolCallDelta {
                                call_id: call_id.clone(),
                                arguments_json_delta: delta.to_string(),
                            },
                        );
                    }
                }
            }
            "response.output_item.done" => {
                let index = jsonh::as_u64(&v, "output_index").unwrap_or(0);
                match self.open_items.remove(&index) {
                    Some(ItemRef::Message(block_id)) => {
                        self.emit(out, ModelEvent::TextEnded { block_id });
                    }
                    Some(ItemRef::Reasoning(block_id)) => {
                        // 加密推理项进入 opaque 载荷（§19.1）
                        if let Some(encrypted) = jsonh::at_str(&v, &["item", "encrypted_content"]) {
                            self.assembler.attach_reasoning_payload(
                                &block_id,
                                json!({"encrypted_content": encrypted}),
                            );
                        }
                        self.emit(out, ModelEvent::ReasoningEnded { block_id });
                    }
                    Some(ItemRef::FunctionCall(call_id)) => {
                        self.emit(out, ModelEvent::ToolCallFinished { call_id });
                    }
                    None => {}
                }
            }
            "response.completed" => {
                let response_body = v
                    .get("response")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let status = jsonh::as_str(&response_body, "status").unwrap_or("completed");
                let has_function_call =
                    jsonh::as_array(&response_body, "output").is_some_and(|o| {
                        o.iter()
                            .any(|i| jsonh::as_str(i, "type") == Some("function_call"))
                    });
                let stop_reason = if has_function_call {
                    StopReason::ToolUse
                } else if status == "incomplete" {
                    match jsonh::at_str(&response_body, &["incomplete_details", "reason"]) {
                        Some("max_output_tokens") => StopReason::MaxTokens,
                        _ => StopReason::EndTurn,
                    }
                } else {
                    StopReason::EndTurn
                };
                let usage = self.extract_usage(&response_body);
                if let Some(u) = usage.clone() {
                    self.emit(out, ModelEvent::UsageUpdated { usage: u });
                }
                let response = GenerateResponse {
                    invocation_id: umber_core::ids::InvocationId::from("engine-owned"),
                    // 内容以流内聚合为准；加密推理项已在 output_item.done 挂接
                    content: self.assembler.to_partial(None).content,
                    stop_reason,
                    usage: usage.unwrap_or_else(Usage::zero),
                    provider_context: ProviderContext::default(),
                };
                out.push(ModelEvent::Completed {
                    response: Box::new(response),
                });
            }
            "response.failed" | "error" => {
                let (code, message) = if kind == "error" {
                    extract_code_message(&v)
                } else {
                    extract_code_message(jsonh::at(&v, &["response", "error"]).unwrap_or(&v))
                };
                out.push(ModelEvent::Failed {
                    error: refine(
                        map_status(500, code.clone(), message.clone()),
                        code.as_deref(),
                        &message,
                    ),
                });
            }
            _ => {} // created / in_progress / content_part.* / reasoning_summary_part.* 等
        }
    }

    fn extract_usage(&self, response_body: &serde_json::Value) -> Option<Usage> {
        let u = response_body.get("usage").filter(|u| !u.is_null())?;
        Some(Usage::new(
            jsonh::as_u64(u, "input_tokens").unwrap_or(0),
            jsonh::as_u64(u, "output_tokens").unwrap_or(0),
            jsonh::at_u64(u, &["output_tokens_details", "reasoning_tokens"]).unwrap_or(0),
            jsonh::at_u64(u, &["input_tokens_details", "cached_tokens"]).unwrap_or(0),
        ))
    }
    #[allow(dead_code)]
    fn saw_content(&self) -> bool {
        self.seen_content
    }
}
