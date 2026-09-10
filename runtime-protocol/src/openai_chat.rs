//! OpenAI Chat Completions 协议 Adapter（总案 §5，M2）。
//!
//! 同时覆盖一切 "OpenAI Compatible" 服务——它们不是新协议，
//! 而是 `openai_chat + CompatibilityProfile`（总案 §5）。
//!
//! 请求映射要点：
//! - `reasoning_effort` 只在显式非默认档位时发送（Minimal/Low/High）
//! - 结构化输出 → `response_format.json_schema`
//! - 缓存走 Provider 自动缓存；块级 `cache_control` 无对应字段，忽略
//! - 工具调用名必须随首个片段到达（OpenAI/DeepSeek 行为），契约如此约定
//!
//! 流解析要点：
//! - `data: [DONE]` 触发 `Completed` 合成（usage 缺省时降级为零值）
//! - 并行 tool_calls 按 `index` 归属重组（总案 §24）
//! - Completed 的 invocation_id 由 Engine 直通时覆写（Canonical 身份归 Runtime）

use std::collections::BTreeMap;
use std::sync::Arc;

use runtime_core::content::ContentBlock;
use runtime_core::error::{ErrorDetail, ModelError};
use runtime_core::event::ModelEvent;
use runtime_core::ids::{BlockId, CallId};
use runtime_core::message::Role;
use runtime_core::request::{GenerateRequest, ReasoningEffort, ResponseFormat, ToolChoice};
use runtime_core::response::{GenerateResponse, ProviderContext, StopReason};
use runtime_core::usage::Usage;
use runtime_credential::{CredentialRef, CredentialStore};
use runtime_engine::{CancelToken, ProviderStream, StreamAssembler};
use runtime_model::deployment::{Deployment, Endpoint, ProtocolKind};
use runtime_provider::{AdapterDescriptor, DiscoveredModel, ProviderAdapter};
use serde_json::json;

use crate::errors::{extract_code_message, map_status, refine};
use crate::join_url;
use crate::jsonh;
use crate::sse::{is_done_marker, SseAssembler};
use crate::transport::{Headers, HttpResponse, HttpTransport};
use crate::worker::ChannelStream;

pub struct OpenAiChatAdapter {
    transport: Arc<dyn HttpTransport>,
}

impl OpenAiChatAdapter {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }

    pub fn chat_completions_url(endpoint: &Endpoint) -> String {
        join_url(&endpoint.url, "chat/completions")
    }

    pub fn models_url(endpoint: &Endpoint) -> String {
        join_url(&endpoint.url, "models")
    }

    fn headers(credentials: &dyn CredentialStore, credential_ref: &CredentialRef) -> Headers {
        let mut headers: Headers = vec![
            ("content-type".into(), "application/json".into()),
            (
                "accept".into(),
                "application/json, text/event-stream".into(),
            ),
        ];
        // 免密网关 / 本地推理服务允许无凭据（总案 §35 Custom 一等公民）
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

impl ProviderAdapter for OpenAiChatAdapter {
    fn describe(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            provider_id: "openai_chat".into(),
            display_name: "OpenAI Chat Completions".into(),
            protocols: vec![ProtocolKind::OpenAiChat],
        }
    }

    fn discover_models(
        &self,
        endpoint: &Endpoint,
        credentials: &dyn CredentialStore,
        credential_ref: &CredentialRef,
    ) -> Result<Vec<DiscoveredModel>, ModelError> {
        let url = Self::models_url(endpoint);
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
        let url = Self::chat_completions_url(endpoint);
        match self.transport.post_stream(
            &url,
            &Self::headers(credentials, credential_ref),
            body.to_string(),
        )? {
            HttpResponse::Sse { lines } => {
                let cancel = CancelToken::new();
                let mut sse = SseAssembler::new();
                let mut parser = ChatStreamParser::new();
                Ok(Box::new(ChannelStream::spawn(
                    cancel,
                    lines,
                    move |line, out| {
                        if let Some(event) = sse.feed_line(line) {
                            if is_done_marker(&event) {
                                parser.finish(out);
                                return false;
                            }
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
    request.validate().map_err(invalid_request)?;

    let mut messages = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::System => {
                let text = message
                    .content
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                messages.push(json!({"role": "system", "content": text}));
            }
            Role::User => {
                messages.push(json!({"role": "user", "content": user_content(&message.content)}));
            }
            Role::Assistant => {
                let text = message
                    .content
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join("");
                let tool_calls: Vec<_> = message
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolCall(c) => Some(json!({
                            "id": c.call_id.as_ref(),
                            "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments_json}
                        })),
                        _ => None,
                    })
                    .collect();
                let mut msg = serde_json::Map::new();
                msg.insert("role".into(), json!("assistant"));
                msg.insert(
                    "content".into(),
                    if text.is_empty() && !tool_calls.is_empty() {
                        serde_json::Value::Null
                    } else {
                        json!(text)
                    },
                );
                // DeepSeek 系：多轮工具调用要求回传 reasoning_content（§19.1 的 openai 形态）
                if let Some(reasoning) = message.content.iter().find_map(|b| match b {
                    ContentBlock::Reasoning(r) if !r.text.is_empty() => Some(r.text.clone()),
                    _ => None,
                }) {
                    msg.insert("reasoning_content".into(), json!(reasoning));
                }
                if !tool_calls.is_empty() {
                    msg.insert("tool_calls".into(), json!(tool_calls));
                }
                messages.push(serde_json::Value::Object(msg));
            }
            Role::Tool => {
                for block in &message.content {
                    if let ContentBlock::ToolResult(r) = block {
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": r.call_id.as_ref(),
                            "content": tool_result_text(r)
                        }));
                    }
                }
            }
        }
    }

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model_id));
    body.insert("messages".into(), json!(messages));
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            json!(request
                .tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema
                    }
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
                json!({"type": "function", "function": {"name": name}}),
            );
        }
    }
    // 统一档位只在显式非默认时发送（§21.1：就近降级并记录 CompatibilityProfile）
    if request.reasoning.effort != ReasoningEffort::Medium {
        let value = match request.reasoning.effort {
            ReasoningEffort::Minimal => "minimal",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        };
        body.insert("reasoning_effort".into(), json!(value));
    }
    let g = &request.generation;
    if let Some(v) = g.max_output_tokens {
        body.insert("max_tokens".into(), json!(v));
    }
    if let Some(v) = g.temperature {
        body.insert("temperature".into(), json!(v));
    }
    if let Some(v) = g.top_p {
        body.insert("top_p".into(), json!(v));
    }
    if !g.stop_sequences.is_empty() {
        body.insert("stop".into(), json!(g.stop_sequences));
    }
    if let Some(v) = g.seed {
        body.insert("seed".into(), json!(v));
    }
    match &request.response_format {
        ResponseFormat::Text => {}
        ResponseFormat::JsonObject => {
            body.insert("response_format".into(), json!({"type": "json_object"}));
        }
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => {
            body.insert(
                "response_format".into(),
                json!({"type": "json_schema", "json_schema": {
                    "name": name, "schema": schema, "strict": strict
                }}),
            );
        }
    }
    body.insert("stream".into(), json!(true));
    body.insert("stream_options".into(), json!({"include_usage": true}));

    Ok(serde_json::Value::Object(body))
}

fn user_content(blocks: &[ContentBlock]) -> serde_json::Value {
    let has_media = blocks.iter().any(|b| {
        matches!(
            b,
            ContentBlock::Image(_) | ContentBlock::Audio(_) | ContentBlock::Video(_)
        )
    });
    if !has_media {
        let text = blocks
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("");
        return json!(text);
    }
    let parts: Vec<serde_json::Value> = blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(json!({"type": "text", "text": t.text})),
            ContentBlock::Image(i) => {
                let url = match &i.source {
                    runtime_core::content::MediaSource::Url { url } => url.clone(),
                    runtime_core::content::MediaSource::Base64 { media_type, data } => {
                        format!("data:{media_type};base64,{data}")
                    }
                };
                Some(json!({"type": "image_url", "image_url": {"url": url}}))
            }
            _ => None,
        })
        .collect();
    json!(parts)
}

fn tool_result_text(
    runtime_core::content::ToolResultBlock {
        is_error, content, ..
    }: &runtime_core::content::ToolResultBlock,
) -> String {
    let text = content
        .iter()
        .filter_map(|b| b.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    if *is_error {
        format!("error: {text}")
    } else {
        text
    }
}

/// 请求级校验失败 → InvalidRequest。
fn invalid_request(e: runtime_core::request::RequestValidationError) -> ModelError {
    ModelError::InvalidRequest(ErrorDetail::new(e.to_string()))
}

/// 流解析状态机：delta → Canonical 事件 + 内部聚合（用于合成 Completed）。
struct ChatStreamParser {
    assembler: StreamAssembler,
    text_open: bool,
    reasoning_open: bool,
    text_block: BlockId,
    reasoning_block: BlockId,
    tool_calls: BTreeMap<u64, CallId>,
    finish: Option<StopReason>,
    usage: Option<Usage>,
}

impl ChatStreamParser {
    fn new() -> Self {
        Self {
            assembler: StreamAssembler::new(),
            text_open: false,
            reasoning_open: false,
            text_block: BlockId::from("chat-text"),
            reasoning_block: BlockId::from("chat-reasoning"),
            tool_calls: BTreeMap::new(),
            finish: None,
            usage: None,
        }
    }

    fn emit(&mut self, out: &mut Vec<ModelEvent>, event: ModelEvent) {
        self.assembler.absorb(&event);
        out.push(event);
    }

    fn line(&mut self, data: &str, out: &mut Vec<ModelEvent>) {
        let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) else {
            return; // 非 JSON 行忽略
        };

        if let Some(u) = chunk.get("usage").filter(|u| !u.is_null()) {
            let usage = Usage::new(
                jsonh::as_u64(u, "prompt_tokens").unwrap_or(0),
                jsonh::as_u64(u, "completion_tokens").unwrap_or(0),
                jsonh::at_u64(u, &["completion_tokens_details", "reasoning_tokens"]).unwrap_or(0),
                jsonh::at_u64(u, &["prompt_tokens_details", "cached_tokens"]).unwrap_or(0),
            );
            self.usage = Some(usage.clone());
            self.emit(out, ModelEvent::UsageUpdated { usage });
        }

        for choice in jsonh::as_array(&chunk, "choices").into_iter().flatten() {
            let delta = choice
                .get("delta")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            if let Some(text) = jsonh::as_str(&delta, "content") {
                if !text.is_empty() {
                    if !self.text_open {
                        self.text_open = true;
                        self.emit(
                            out,
                            ModelEvent::TextStarted {
                                block_id: self.text_block.clone(),
                            },
                        );
                    }
                    self.emit(
                        out,
                        ModelEvent::TextDelta {
                            block_id: self.text_block.clone(),
                            delta: text.to_string(),
                        },
                    );
                }
            }

            if let Some(reasoning) = jsonh::as_str(&delta, "reasoning_content") {
                if !reasoning.is_empty() {
                    if !self.reasoning_open {
                        self.reasoning_open = true;
                        self.emit(
                            out,
                            ModelEvent::ReasoningStarted {
                                block_id: self.reasoning_block.clone(),
                            },
                        );
                    }
                    self.emit(
                        out,
                        ModelEvent::ReasoningDelta {
                            block_id: self.reasoning_block.clone(),
                            delta: reasoning.to_string(),
                        },
                    );
                }
            }

            if let Some(tool_calls) = jsonh::as_array(&delta, "tool_calls") {
                for tc in tool_calls {
                    let index = jsonh::as_u64(tc, "index").unwrap_or(0);
                    let call_id = match self.tool_calls.get(&index) {
                        Some(id) => id.clone(),
                        None => {
                            let id = jsonh::as_str(tc, "id")
                                .map(CallId::from)
                                .unwrap_or_else(|| CallId::from(format!("call-{index}")));
                            let name = jsonh::at_str(tc, &["function", "name"])
                                .unwrap_or_default()
                                .to_string();
                            self.emit(
                                out,
                                ModelEvent::ToolCallStarted {
                                    call_id: id.clone(),
                                    name,
                                },
                            );
                            self.tool_calls.insert(index, id.clone());
                            id
                        }
                    };
                    if let Some(args) = jsonh::at_str(tc, &["function", "arguments"]) {
                        if !args.is_empty() {
                            self.emit(
                                out,
                                ModelEvent::ToolCallDelta {
                                    call_id,
                                    arguments_json_delta: args.to_string(),
                                },
                            );
                        }
                    }
                }
            }

            if let Some(finish) = jsonh::as_str(choice, "finish_reason") {
                self.finish = Some(match finish {
                    "length" => StopReason::MaxTokens,
                    "tool_calls" | "function_call" => StopReason::ToolUse,
                    "content_filter" => StopReason::SafetyBlocked,
                    _ => StopReason::EndTurn,
                });
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<ModelEvent>) {
        if self.text_open {
            self.emit(
                out,
                ModelEvent::TextEnded {
                    block_id: self.text_block.clone(),
                },
            );
        }
        if self.reasoning_open {
            self.emit(
                out,
                ModelEvent::ReasoningEnded {
                    block_id: self.reasoning_block.clone(),
                },
            );
        }
        let call_ids: Vec<CallId> = self.tool_calls.values().cloned().collect();
        for call_id in call_ids {
            self.emit(out, ModelEvent::ToolCallFinished { call_id });
        }
        let response = GenerateResponse {
            // Engine 直通时覆写为真实 invocation_id（Canonical 身份归 Runtime）
            invocation_id: runtime_core::ids::InvocationId::from("engine-owned"),
            content: self.assembler.to_partial(None).content,
            stop_reason: self.finish.unwrap_or(StopReason::EndTurn),
            usage: self.usage.clone().unwrap_or_else(Usage::zero),
            provider_context: ProviderContext::default(),
        };
        out.push(ModelEvent::Completed {
            response: Box::new(response),
        });
    }
}
