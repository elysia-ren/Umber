//! Google Gemini 协议 Adapter（总案 §4，M5）。
//!
//! 请求映射要点：
//! - system → `systemInstruction.parts`；assistant 角色为 `model`
//! - 工具调用参数是 JSON 对象（非字符串文本），重组后需 parse
//! - ToolResult → `{functionResponse:{name, response:{result}}}`——
//!   Gemini 没有 call_id，按函数名配对
//! - 结构化输出 → `generationConfig.responseMimeType/Schema`（子集映射，
//!   超集字段丢弃并记录 CompatibilityProfile，总案 §18.1）
//! - reasoning 档位 → `thinkingConfig.thinkingBudget`（§21.1）
//!
//! 流解析要点：`alt=sse`，纯 data JSON（无事件名）；finishReason ∈
//! STOP / MAX_TOKENS / SAFETY / RECITATION；promptFeedback.blockReason
//! → ContentFiltered。

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
use crate::jsonh;
use crate::sse::SseAssembler;
use crate::transport::{Headers, HttpResponse, HttpTransport};
use crate::trim_trailing_action;
use crate::worker::ChannelStream;

pub struct GeminiAdapter {
    transport: Arc<dyn HttpTransport>,
}

/// 统一推理档位 → thinkingBudget 映射（总案 §21.1）。
pub fn effort_to_thinking_budget(effort: ReasoningEffort) -> i64 {
    match effort {
        ReasoningEffort::Minimal => 0,
        ReasoningEffort::Low => 1024,
        ReasoningEffort::Medium => 8192,
        ReasoningEffort::High => 24576,
    }
}

impl GeminiAdapter {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }

    pub fn stream_url(endpoint: &Endpoint, model_id: &str) -> String {
        let base = trim_trailing_action(&endpoint.url, "");
        format!("{base}/v1beta/models/{model_id}:streamGenerateContent?alt=sse")
    }

    pub fn models_url(endpoint: &Endpoint) -> String {
        format!("{}/v1beta/models", endpoint.url.trim_end_matches('/'))
    }

    fn headers(credentials: &dyn CredentialStore, credential_ref: &CredentialRef) -> Headers {
        let mut headers: Headers = vec![("content-type".into(), "application/json".into())];
        if let Ok(Some(secret)) = credentials.get(credential_ref) {
            if !secret.expose().is_empty() {
                headers.push(("x-goog-api-key".into(), secret.expose().to_string()));
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

impl ProviderAdapter for GeminiAdapter {
    fn describe(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            provider_id: "google".into(),
            display_name: "Google Gemini".into(),
            protocols: vec![ProtocolKind::Gemini],
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
                if let Some(items) = jsonh::as_array(&body, "models") {
                    for item in items {
                        // name 形如 "models/gemini-2.5-flash"
                        if let Some(name) = jsonh::as_str(item, "name") {
                            let supports_generate =
                                jsonh::as_array(item, "supportedGenerationMethods")
                                    .map(|m| {
                                        m.iter()
                                            .filter_map(|v| v.as_str())
                                            .any(|s| s == "generateContent")
                                    })
                                    .unwrap_or(true);
                            if !supports_generate {
                                continue;
                            }
                            let model_id = name.strip_prefix("models/").unwrap_or(name);
                            models.push(DiscoveredModel {
                                model_id: model_id.to_string(),
                                display_name: jsonh::as_str(item, "displayName")
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
        let url = Self::stream_url(endpoint, &deployment.model_id);
        match self.transport.post_stream(
            &url,
            &Self::headers(credentials, credential_ref),
            body.to_string(),
        )? {
            HttpResponse::Sse { lines } => {
                let cancel = CancelToken::new();
                let mut sse = SseAssembler::new();
                let mut parser = GeminiStreamParser::new();
                // Gemini 以流 EOF 收尾；追加哨兵行触发 Completed 合成（§23.1）
                const EOF_SENTINEL: &str = "\u{0}umer-eof";
                let mut eof_sent = false;
                let mut lines = lines;
                let chained = std::iter::from_fn(move || match lines.next() {
                    Some(l) => Some(l),
                    None => {
                        if eof_sent {
                            None
                        } else {
                            eof_sent = true;
                            Some(Ok(EOF_SENTINEL.to_string()))
                        }
                    }
                });
                Ok(Box::new(ChannelStream::spawn(
                    cancel,
                    Box::new(chained),
                    move |line, out| {
                        if line == EOF_SENTINEL {
                            parser.finish(out);
                            return false;
                        }
                        if let Some(event) = sse.feed_line(line) {
                            parser.line(&event.data, out);
                        }
                        true
                    },
                )))
            }
            HttpResponse::Json { status, body } if !(200..300).contains(&status) => {
                Err(Self::map_error(status, &body))
            }
            HttpResponse::Json { status: _, body } => {
                // 2xx JSON 体：Gemini 在非流式 200 响应里返回拦截等结果，
                // 直接送入解析器（不走 SSE 装配），EOF 哨兵触发 Completed
                let cancel = CancelToken::new();
                let mut parser = GeminiStreamParser::new();
                let body_line = body.to_string();
                let mut lines: Box<dyn Iterator<Item = Result<String, ModelError>> + Send> =
                    Box::new(std::iter::once(Ok(body_line)));
                let mut eof_sent = false;
                const EOF_SENTINEL: &str = "\u{0}umer-eof-json";
                let chained = std::iter::from_fn(move || match lines.next() {
                    Some(l) => Some(l),
                    None => {
                        if eof_sent {
                            None
                        } else {
                            eof_sent = true;
                            Some(Ok(EOF_SENTINEL.to_string()))
                        }
                    }
                });
                Ok(Box::new(ChannelStream::spawn(
                    cancel,
                    Box::new(chained),
                    move |line, out| {
                        if line == EOF_SENTINEL {
                            parser.finish(out);
                            return false;
                        }
                        parser.line(line, out);
                        true
                    },
                )))
            }
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

    let mut system_parts: Vec<serde_json::Value> = Vec::new();
    let mut contents: Vec<serde_json::Value> = Vec::new();
    // Gemini 工具结果按函数名配对：先收集 call_id → name 映射
    let mut call_names: BTreeMap<String, String> = BTreeMap::new();
    for message in &request.messages {
        for b in &message.content {
            if let ContentBlock::ToolCall(c) = b {
                call_names.insert(c.call_id.to_string(), c.name.clone());
            }
        }
    }

    for message in &request.messages {
        let role = match message.role {
            Role::User | Role::Tool => "user",
            Role::Assistant => "model",
            Role::System => {
                for b in &message.content {
                    if let Some(t) = b.as_text() {
                        system_parts.push(json!({"text": t}));
                    }
                }
                continue;
            }
        };
        let mut parts: Vec<serde_json::Value> = Vec::new();
        for b in &message.content {
            match b {
                ContentBlock::Text(t) => parts.push(json!({"text": t.text})),
                ContentBlock::Image(i) => match &i.source {
                    umber_core::content::MediaSource::Url { url } => {
                        parts.push(json!({"fileData": {"mimeType": "image/*", "fileUri": url}}));
                    }
                    umber_core::content::MediaSource::Base64 { media_type, data } => {
                        parts.push(json!({"inlineData": {"mimeType": media_type, "data": data}}));
                    }
                },
                ContentBlock::ToolCall(c) => {
                    let arguments: serde_json::Value =
                        serde_json::from_str(&c.arguments_json).unwrap_or(json!({}));
                    parts.push(json!({"functionCall": {"name": c.name, "args": arguments}}));
                }
                ContentBlock::ToolResult(r) => {
                    let result_text = r
                        .content
                        .iter()
                        .filter_map(|b| b.as_text())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let name = call_names
                        .get(r.call_id.as_ref())
                        .cloned()
                        .unwrap_or_else(|| r.call_id.to_string());
                    parts.push(json!({
                        "functionResponse": {
                            "name": name,
                            "response": {"result": result_text}
                        }
                    }));
                }
                _ => {}
            }
        }
        if !parts.is_empty() {
            contents.push(json!({"role": role, "parts": parts}));
        }
    }

    let mut body = serde_json::Map::new();
    let _ = model_id; // 模型名走 URL 路径
    body.insert("contents".into(), json!(contents));
    if !system_parts.is_empty() {
        body.insert("systemInstruction".into(), json!({"parts": system_parts}));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            json!([{"functionDeclarations": request
                .tools
                .iter()
                .map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                }))
                .collect::<Vec<_>>()}]),
        );
    }
    match &request.tool_choice {
        ToolChoice::Auto => {
            body.insert(
                "toolConfig".into(),
                json!({"functionCallingConfig": {"mode": "AUTO"}}),
            );
        }
        ToolChoice::None => {
            body.insert(
                "toolConfig".into(),
                json!({"functionCallingConfig": {"mode": "NONE"}}),
            );
        }
        ToolChoice::Required => {
            body.insert(
                "toolConfig".into(),
                json!({"functionCallingConfig": {"mode": "ANY"}}),
            );
        }
        ToolChoice::Specific { name } => {
            body.insert(
                "toolConfig".into(),
                json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": [name]}}),
            );
        }
    }
    if request.reasoning.effort != ReasoningEffort::Medium {
        body.insert(
            "generationConfig".into(),
            json!({"thinkingConfig": {"thinkingBudget": effort_to_thinking_budget(request.reasoning.effort)}}),
        );
    }
    let mut gen_config = serde_json::Map::new();
    let g = &request.generation;
    if let Some(v) = g.max_output_tokens {
        gen_config.insert("maxOutputTokens".into(), json!(v));
    }
    if let Some(v) = g.temperature {
        gen_config.insert("temperature".into(), json!(v));
    }
    if let Some(v) = g.top_p {
        gen_config.insert("topP".into(), json!(v));
    }
    if !g.stop_sequences.is_empty() {
        gen_config.insert("stopSequences".into(), json!(g.stop_sequences));
    }
    match &request.response_format {
        ResponseFormat::Text => {}
        ResponseFormat::JsonObject => {
            gen_config.insert("responseMimeType".into(), json!("application/json"));
        }
        ResponseFormat::JsonSchema {
            name: _,
            schema,
            strict: _,
        } => {
            gen_config.insert("responseMimeType".into(), json!("application/json"));
            // 子集映射；不兼容字段已在 to_gemini_schema 中丢弃（§18.1）
            gen_config.insert("responseSchema".into(), to_gemini_schema(schema));
        }
    }
    if !gen_config.is_empty() {
        match body.get_mut("generationConfig") {
            Some(existing) => {
                if let (serde_json::Value::Object(a), serde_json::Value::Object(b)) =
                    (existing, serde_json::Value::Object(gen_config.clone()))
                {
                    for (k, v) in b {
                        a.insert(k, v);
                    }
                }
            }
            None => {
                body.insert(
                    "generationConfig".into(),
                    serde_json::Value::Object(gen_config),
                );
            }
        }
    }

    Ok(serde_json::Value::Object(body))
}

/// Canonical JSON Schema → Gemini responseSchema 子集映射（总案 §18.1）。
/// 超集字段（additionalProperties / $schema / oneOf 等）丢弃——
/// 调用方应在 CompatibilityProfile 中将 structured_output 标 partial。
pub fn to_gemini_schema(schema: &serde_json::Value) -> serde_json::Value {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return json!({}),
    };
    let mut out = serde_json::Map::new();
    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
        out.insert("type".into(), json!(t.to_uppercase()));
    }
    for key in ["format", "description", "enum", "required"] {
        if let Some(v) = obj.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    if let Some(items) = obj.get("items") {
        out.insert("items".into(), to_gemini_schema(items));
    }
    if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
        let mapped: serde_json::Map<String, serde_json::Value> = props
            .iter()
            .map(|(k, v)| (k.clone(), to_gemini_schema(v)))
            .collect();
        out.insert("properties".into(), serde_json::Value::Object(mapped));
    }
    serde_json::Value::Object(out)
}

/// 流解析状态机（alt=sse，纯 data JSON）。
struct GeminiStreamParser {
    assembler: StreamAssembler,
    text_open: bool,
    text_block: BlockId,
    tool_seq: u64,
    saw_tool_call: bool,
    finish: Option<StopReason>,
    usage: Option<Usage>,
}

impl GeminiStreamParser {
    fn new() -> Self {
        Self {
            assembler: StreamAssembler::new(),
            text_open: false,
            text_block: BlockId::from("gemini-text"),
            tool_seq: 0,
            saw_tool_call: false,
            finish: None,
            usage: None,
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

        // 无 candidates 且被安全拦截 → ContentFiltered（§28）
        if v.get("candidates").map(|c| c.is_null()).unwrap_or(true) {
            if let Some(reason) = jsonh::at_str(&v, &["promptFeedback", "blockReason"]) {
                let mut d = ErrorDetail::new(format!("prompt blocked: {reason}"));
                d.provider_code = Some(reason.to_string());
                out.push(ModelEvent::Failed {
                    error: ModelError::ContentFiltered(d),
                });
            }
            return;
        }

        if let Some(u) = v.get("usageMetadata").filter(|u| !u.is_null()) {
            let usage = Usage::new(
                jsonh::as_u64(u, "promptTokenCount").unwrap_or(0),
                jsonh::as_u64(u, "candidatesTokenCount").unwrap_or(0),
                jsonh::as_u64(u, "thoughtsTokenCount").unwrap_or(0),
                jsonh::as_u64(u, "cachedContentTokenCount").unwrap_or(0),
            );
            self.usage = Some(usage.clone());
            self.emit(out, ModelEvent::UsageUpdated { usage });
        }

        let Some(candidates) = jsonh::as_array(&v, "candidates") else {
            return;
        };
        let Some(candidate) = candidates.first() else {
            return;
        };
        if let Some(parts) = jsonh::at(candidate, &["content", "parts"]).and_then(|p| p.as_array())
        {
            for part in parts {
                if let Some(text) = jsonh::as_str(part, "text") {
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
                if let Some(call) = part.get("functionCall") {
                    // Gemini 一次性下发完整函数调用
                    let name = jsonh::as_str(call, "name").unwrap_or_default().to_string();
                    let args = call.get("args").cloned().unwrap_or(json!({}));
                    self.tool_seq += 1;
                    self.saw_tool_call = true;
                    let call_id = CallId::from(format!("call-{}", self.tool_seq));
                    self.emit(
                        out,
                        ModelEvent::ToolCallStarted {
                            call_id: call_id.clone(),
                            name,
                        },
                    );
                    self.emit(
                        out,
                        ModelEvent::ToolCallDelta {
                            call_id: call_id.clone(),
                            arguments_json_delta: args.to_string(),
                        },
                    );
                    self.emit(out, ModelEvent::ToolCallFinished { call_id });
                }
            }
        }
        if let Some(finish) = jsonh::as_str(candidate, "finishReason") {
            self.finish = Some(match finish {
                "MAX_TOKENS" => StopReason::MaxTokens,
                "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST" => {
                    StopReason::SafetyBlocked
                }
                _ => StopReason::EndTurn,
            });
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
        // Gemini 以带 finishReason 的最后一个 chunk 收尾；
        // STOP 可伴随 functionCall 出现，工具调用优先于 finishReason（§27）
        let stop_reason = if self.saw_tool_call {
            StopReason::ToolUse
        } else {
            self.finish.unwrap_or(StopReason::EndTurn)
        };
        let response = GenerateResponse {
            invocation_id: umber_core::ids::InvocationId::from("engine-owned"),
            content: self.assembler.to_partial(None).content,
            stop_reason,
            usage: self.usage.clone().unwrap_or_else(Usage::zero),
            provider_context: ProviderContext::default(),
        };
        out.push(ModelEvent::Completed {
            response: Box::new(response),
        });
    }
}
