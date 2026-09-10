//! Anthropic Messages 协议 Adapter（总案 §4，M3）。
//!
//! 请求映射要点：
//! - system 独立于 messages；max_tokens 必填（缺省 4096）
//! - ToolResult → user 消息内的 `tool_result` 块
//! - Reasoning 统一档位 → `thinking.budget_tokens`（仅在显式非默认档位时发送）
//! - Reasoning.provider_payload 存放 thinking 的 `signature`；
//!   多轮回放时重组为 `{"type":"thinking","thinking":…,"signature":…}` 原样回传
//!   （总案 §19.1：签名不回传，Provider 会拒绝多轮工具调用）
//! - 块级 `cache_control` → 显式缓存断点（总案 §20）
//!
//! 流解析要点：SSE 带事件名（message_start / content_block_* / message_delta /
//! message_stop / ping / error），signature_delta 累积不产生事件。

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

pub struct AnthropicAdapter {
    transport: Arc<dyn HttpTransport>,
}

/// 统一推理档位 → budget_tokens 映射（总案 §21.1）。
pub fn effort_to_budget(effort: ReasoningEffort) -> u32 {
    match effort {
        ReasoningEffort::Minimal => 1024,
        ReasoningEffort::Low => 2048,
        ReasoningEffort::Medium => 8192,
        ReasoningEffort::High => 24576,
    }
}

impl AnthropicAdapter {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }

    pub fn messages_url(endpoint: &Endpoint) -> String {
        join_url(&endpoint.url, "v1/messages")
    }

    pub fn models_url(endpoint: &Endpoint) -> String {
        join_url(&endpoint.url, "v1/models")
    }

    fn headers(credentials: &dyn CredentialStore, credential_ref: &CredentialRef) -> Headers {
        let mut headers: Headers = vec![
            ("content-type".into(), "application/json".into()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ];
        if let Ok(Some(secret)) = credentials.get(credential_ref) {
            if !secret.expose().is_empty() {
                headers.push(("x-api-key".into(), secret.expose().to_string()));
            }
        }
        headers
    }

    fn map_error(status: u16, body: &serde_json::Value) -> ModelError {
        let (code, message) = extract_code_message(body);
        let base = if status == 529 {
            ModelError::Overloaded(ErrorDetail {
                provider_code: code.clone(),
                ..ErrorDetail::new(message.clone())
            })
        } else {
            map_status(status, code.clone(), message.clone())
        };
        refine(base, code.as_deref(), &message)
    }
}

impl ProviderAdapter for AnthropicAdapter {
    fn describe(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            provider_id: "anthropic".into(),
            display_name: "Anthropic Messages".into(),
            protocols: vec![ProtocolKind::AnthropicMessages],
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
        let url = Self::messages_url(endpoint);
        match self.transport.post_stream(
            &url,
            &Self::headers(credentials, credential_ref),
            body.to_string(),
        )? {
            HttpResponse::Sse { lines } => {
                let cancel = CancelToken::new();
                let mut sse = SseAssembler::new();
                let mut parser = AnthropicStreamParser::new();
                Ok(Box::new(ChannelStream::spawn(
                    cancel,
                    lines,
                    move |line, out| match sse.feed_line(line) {
                        Some(event) => parser.line(event.name.as_deref(), &event.data, out),
                        None => true,
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

    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for message in &request.messages {
        match message.role {
            Role::System => {
                for b in &message.content {
                    if let ContentBlock::Text(t) = b {
                        // 块级缓存断点：System 部分是最佳缓存前缀（总案 §20）
                        let mut block = json!({"type": "text", "text": t.text});
                        if let Some(umber_core::content::CacheControl::Ephemeral) = t.cache_control
                        {
                            block["cache_control"] = json!({"type": "ephemeral"});
                        }
                        system_parts.push(serde_json::to_string(&block).unwrap());
                    }
                }
            }
            Role::User | Role::Tool => {
                // Tool 结果在 Anthropic 里是 user 消息内的 tool_result 块
                let mut content: Vec<serde_json::Value> = Vec::new();
                for b in &message.content {
                    match b {
                        ContentBlock::Text(t) => {
                            content.push(json!({"type": "text", "text": t.text}))
                        }
                        ContentBlock::Image(i) => {
                            content.push(image_block(i));
                        }
                        ContentBlock::ToolResult(r) => {
                            let inner: Vec<serde_json::Value> = r
                                .content
                                .iter()
                                .filter_map(|b| {
                                    b.as_text().map(|t| json!({"type": "text", "text": t}))
                                })
                                .collect();
                            content.push(json!({
                                "type": "tool_result",
                                "tool_use_id": r.call_id.as_ref(),
                                "is_error": r.is_error,
                                "content": inner
                            }));
                        }
                        _ => {}
                    }
                }
                if !content.is_empty() {
                    messages.push(json!({"role": "user", "content": content}));
                }
            }
            Role::Assistant => {
                let mut content: Vec<serde_json::Value> = Vec::new();
                for b in &message.content {
                    match b {
                        ContentBlock::Text(t) => {
                            content.push(json!({"type": "text", "text": t.text}))
                        }
                        // thinking + signature 必须原样回传，否则多轮工具调用被拒（§19.1）
                        ContentBlock::Reasoning(r) => {
                            if !r.text.is_empty() {
                                let signature = r
                                    .provider_payload
                                    .as_ref()
                                    .and_then(|p| p.get("signature"))
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("");
                                content.push(json!({
                                    "type": "thinking",
                                    "thinking": r.text,
                                    "signature": signature
                                }));
                            }
                        }
                        ContentBlock::ToolCall(c) => {
                            let arguments: serde_json::Value =
                                serde_json::from_str(&c.arguments_json)
                                    .unwrap_or(serde_json::Value::Null);
                            content.push(json!({
                                "type": "tool_use",
                                "id": c.call_id.as_ref(),
                                "name": c.name,
                                "input": arguments
                            }));
                        }
                        _ => {}
                    }
                }
                if !content.is_empty() {
                    messages.push(json!({"role": "assistant", "content": content}));
                }
            }
        }
    }

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model_id));
    body.insert(
        "max_tokens".into(),
        json!(request.generation.max_output_tokens.unwrap_or(4096)),
    );
    if !system_parts.is_empty() {
        let blocks: Vec<serde_json::Value> = system_parts
            .iter()
            .map(|s| serde_json::from_str(s).unwrap_or(serde_json::Value::Null))
            .collect();
        body.insert("system".into(), json!(blocks));
    }
    body.insert("messages".into(), json!(messages));
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            json!(request
                .tools
                .iter()
                .map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema
                }))
                .collect::<Vec<_>>()),
        );
    }
    match &request.tool_choice {
        ToolChoice::Auto => {}
        ToolChoice::None => {} // Anthropic 无 none；不发 tool_choice
        ToolChoice::Required => {
            body.insert("tool_choice".into(), json!({"type": "any"}));
        }
        ToolChoice::Specific { name } => {
            body.insert("tool_choice".into(), json!({"type": "tool", "name": name}));
        }
    }
    if request.reasoning.effort != ReasoningEffort::Medium {
        body.insert(
            "thinking".into(),
            json!({
                "type": "enabled",
                "budget_tokens": effort_to_budget(request.reasoning.effort)
            }),
        );
    }
    let g = &request.generation;
    if let Some(v) = g.temperature {
        body.insert("temperature".into(), json!(v));
    }
    if let Some(v) = g.top_p {
        body.insert("top_p".into(), json!(v));
    }
    if !g.stop_sequences.is_empty() {
        body.insert("stop_sequences".into(), json!(g.stop_sequences));
    }
    match &request.response_format {
        ResponseFormat::Text => {}
        // Anthropic 无原生结构化输出；由 Adapter 合成内部工具承载 schema（§18.1）
        ResponseFormat::JsonObject | ResponseFormat::JsonSchema { .. } => {
            return Err(ModelError::Unsupported(ErrorDetail::new(
                "structured output requires the internal tool synthesis path (not yet enabled for anthropic)",
            )));
        }
    }
    body.insert("stream".into(), json!(true));

    Ok(serde_json::Value::Object(body))
}

fn image_block(i: &umber_core::content::ImageBlock) -> serde_json::Value {
    match &i.source {
        umber_core::content::MediaSource::Url { url } => {
            json!({"type": "image", "source": {"type": "url", "url": url}})
        }
        umber_core::content::MediaSource::Base64 { media_type, data } => {
            json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}})
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    Text,
    Reasoning,
    ToolCall,
}

/// 流解析状态机（Anthropic SSE 按事件名 + index 双归属）。
struct AnthropicStreamParser {
    assembler: StreamAssembler,
    blocks: BTreeMap<u64, (BlockKind, BlockId, Option<CallId>)>,
    signatures: BTreeMap<u64, String>,
    input_tokens: u64,
    output_tokens: u64,
    finish: Option<StopReason>,
}

impl AnthropicStreamParser {
    fn new() -> Self {
        Self {
            assembler: StreamAssembler::new(),
            blocks: BTreeMap::new(),
            signatures: BTreeMap::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish: None,
        }
    }

    fn emit(&mut self, out: &mut Vec<ModelEvent>, event: ModelEvent) {
        self.assembler.absorb(&event);
        out.push(event);
    }

    fn block_id(index: u64) -> BlockId {
        BlockId::from(format!("b{index}"))
    }

    fn line(&mut self, name: Option<&str>, data: &str, out: &mut Vec<ModelEvent>) -> bool {
        let Ok(event_value) = serde_json::from_str::<serde_json::Value>(data) else {
            return true;
        };
        match name.unwrap_or("") {
            "message_start" => {
                self.input_tokens =
                    jsonh::at_u64(&event_value, &["message", "usage", "input_tokens"]).unwrap_or(0);
            }
            "content_block_start" => {
                let index = jsonh::as_u64(&event_value, "index").unwrap_or(0);
                let kind = jsonh::at_str(&event_value, &["content_block", "type"]).unwrap_or("");
                let block_id = Self::block_id(index);
                match kind {
                    "text" => {
                        self.blocks
                            .insert(index, (BlockKind::Text, block_id.clone(), None));
                        self.emit(out, ModelEvent::TextStarted { block_id });
                    }
                    "thinking" => {
                        self.blocks
                            .insert(index, (BlockKind::Reasoning, block_id.clone(), None));
                        self.emit(out, ModelEvent::ReasoningStarted { block_id });
                    }
                    "tool_use" => {
                        let call_id = jsonh::at_str(&event_value, &["content_block", "id"])
                            .map(CallId::from)
                            .unwrap_or_else(|| CallId::from(format!("call-{index}")));
                        let tool_name = jsonh::at_str(&event_value, &["content_block", "name"])
                            .unwrap_or_default();
                        self.blocks.insert(
                            index,
                            (BlockKind::ToolCall, block_id.clone(), Some(call_id.clone())),
                        );
                        self.emit(
                            out,
                            ModelEvent::ToolCallStarted {
                                call_id,
                                name: tool_name.to_string(),
                            },
                        );
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let index = jsonh::as_u64(&event_value, "index").unwrap_or(0);
                let delta_type = jsonh::at_str(&event_value, &["delta", "type"]).unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = jsonh::at_str(&event_value, &["delta", "text"]) {
                            self.emit(
                                out,
                                ModelEvent::TextDelta {
                                    block_id: Self::block_id(index),
                                    delta: text.to_string(),
                                },
                            );
                        }
                    }
                    "thinking_delta" => {
                        if let Some(t) = jsonh::at_str(&event_value, &["delta", "thinking"]) {
                            self.emit(
                                out,
                                ModelEvent::ReasoningDelta {
                                    block_id: Self::block_id(index),
                                    delta: t.to_string(),
                                },
                            );
                        }
                    }
                    "signature_delta" => {
                        if let Some(sig) = jsonh::at_str(&event_value, &["delta", "signature"]) {
                            self.signatures
                                .entry(index)
                                .and_modify(|s| s.push_str(sig))
                                .or_insert_with(|| sig.to_string());
                        }
                    }
                    "input_json_delta" => {
                        if let Some(args) = jsonh::at_str(&event_value, &["delta", "partial_json"])
                        {
                            if let Some((_, _, Some(call_id))) = self.blocks.get(&index) {
                                self.emit(
                                    out,
                                    ModelEvent::ToolCallDelta {
                                        call_id: call_id.clone(),
                                        arguments_json_delta: args.to_string(),
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = jsonh::as_u64(&event_value, "index").unwrap_or(0);
                let block_id = Self::block_id(index);
                match self.blocks.get(&index).map(|(k, _, c)| (*k, c.clone())) {
                    Some((BlockKind::Text, _)) => {
                        self.emit(out, ModelEvent::TextEnded { block_id });
                    }
                    Some((BlockKind::Reasoning, _)) => {
                        // thinking + signature 组装为 opaque 载荷（§19.1）
                        if let Some(sig) = self.signatures.get(&index) {
                            self.assembler
                                .attach_reasoning_payload(&block_id, json!({"signature": sig}));
                        }
                        self.emit(out, ModelEvent::ReasoningEnded { block_id });
                    }
                    Some((BlockKind::ToolCall, Some(call_id))) => {
                        self.emit(out, ModelEvent::ToolCallFinished { call_id });
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(sr) = jsonh::at_str(&event_value, &["delta", "stop_reason"]) {
                    self.finish = Some(match sr {
                        "max_tokens" => StopReason::MaxTokens,
                        "tool_use" => StopReason::ToolUse,
                        "refusal" => StopReason::Refusal,
                        _ => StopReason::EndTurn,
                    });
                }
                if let Some(o) = jsonh::at_u64(&event_value, &["usage", "output_tokens"]) {
                    self.output_tokens = o;
                    self.emit(
                        out,
                        ModelEvent::UsageUpdated {
                            usage: Usage::new(self.input_tokens, self.output_tokens, 0, 0),
                        },
                    );
                }
            }
            "message_stop" => {
                let response = GenerateResponse {
                    invocation_id: umber_core::ids::InvocationId::from("engine-owned"),
                    content: self.assembler.to_partial(None).content,
                    stop_reason: self.finish.unwrap_or(StopReason::EndTurn),
                    usage: Usage::new(self.input_tokens, self.output_tokens, 0, 0),
                    provider_context: ProviderContext::default(),
                };
                out.push(ModelEvent::Completed {
                    response: Box::new(response),
                });
                return false;
            }
            "error" => {
                let (code, message) = extract_code_message(&event_value);
                let err = refine(
                    map_status(500, code.clone(), message.clone()),
                    code.as_deref(),
                    &message,
                );
                out.push(ModelEvent::Failed { error: err });
                return false;
            }
            _ => {} // ping 等
        }
        true
    }
}
