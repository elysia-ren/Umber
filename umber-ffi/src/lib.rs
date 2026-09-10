//! Stable C ABI — 拉取式流（总案 §41.6 §50）。
//!
//! ABI 形态（`contract-v1` 冻结）：
//! - 拉取式：`runtime_stream_next` 阻塞等待 + 超时；不向 C 暴露 async/future
//! - 错误码：正数 = 流状态（事件 / 关闭 / 暂无数据），负数 = 错误
//! - 句柄式内存：谁分配谁释放——事件 JSON 由 `runtime_string_free` 释放
//! - panic 不穿越边界：所有入口 catch_unwind → 错误码
//! - 字符串一律 UTF-8 + 显式长度
//! - ABI 版本：major << 16 | minor；宿主握手校验 major
//!
//! # 事件源（0.2 起）
//!
//! 1. **已配置的 Deployment**：宿主先调 `runtime_set_deployment` +
//!    `runtime_set_credential`（可选 `runtime_load_catalog`），再
//!    `runtime_stream_open`。此时走**真实链路**：
//!    umber-protocol 的协议 Adapter → 真实 HTTP/SSE 传输 → Engine 生命周期
//!    （终结保证 / 四段超时 / 重试 / 取消）。
//!    `GenerateRequest.model` 必须等于部署配置里的 `id`。
//! 2. **demo 流**：仅在宿主显式调用 `runtime_set_demo(rt, 1)` 后可用，
//!    是内置的假事件源，**只用于验证 ABI 形态**（握手 / 拉取 / WOULD_BLOCK /
//!    所有权 / NULL 安全）。
//!
//! 未配置且未开 demo 时，`runtime_stream_open` 返回
//! `UMER_ERR_NOT_CONFIGURED`——**绝不静默返回假数据**。
//!
//! `ProtocolKind::ProviderNative` 暂无独立实现，按 openai_chat 形状处理
//! （与参考宿主 `umber-ui-egui` 一致）。

#![deny(unsafe_op_in_unsafe_fn)]

use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use umber_core::content::{TextBlock, ToolCallBlock};
use umber_core::error::ModelError;
use umber_core::event::{ModelEvent, SequencedEvent};
use umber_core::ids::{BlockId, CallId, DeploymentId, InvocationId};
use umber_core::request::GenerateRequest;
use umber_core::response::{GenerateResponse, ProviderContext, StopReason};
use umber_core::usage::Usage;
use umber_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use umber_engine::{
    run_invocation, CancelToken, ProviderStream, RetryPolicy, SourceFactory, TimeoutPolicy,
};
use umber_model::deployment::{Deployment, Endpoint, ProtocolKind};
use umber_model::Catalog;
use umber_protocol::{
    AnthropicAdapter, GeminiAdapter, HttpConfig, HttpTransport, OpenAiChatAdapter,
    OpenAiResponsesAdapter, RealHttpTransport,
};
use umber_provider::ProviderAdapter;

// ---------- 常量契约 ----------

pub const RUNTIME_ABI_MAJOR: u32 = 0;
pub const RUNTIME_ABI_MINOR: u32 = 2;

pub const UMER_OK: i32 = 0;
pub const UMER_ERR_NULL_ARGUMENT: i32 = -1;
pub const UMER_ERR_ABI_MISMATCH: i32 = -2;
pub const UMER_ERR_OPEN_FAILED: i32 = -3;
pub const UMER_ERR_INTERNAL: i32 = -4;
/// `runtime_stream_open` 的目标 Deployment 未配置（且未开启 demo）。
pub const UMER_ERR_NOT_CONFIGURED: i32 = -5;
/// Catalog 读取 / 解析 / 格式版本校验失败。
pub const UMER_ERR_CATALOG: i32 = -6;

/// `runtime_stream_next` / `runtime_status` 状态：数据已交付。
pub const UMER_EVENT: i32 = 1;
/// 流已终结（终结合成事件已在之前交付）。
pub const UMER_CLOSED: i32 = 2;
/// 等待超时，流可能仍有后续事件。
pub const UMER_WOULD_BLOCK: i32 = 3;

// ---------- Opaque 句柄 ----------

/// 一个已配置的 Deployment 及其 Endpoint 与凭据引用。
#[derive(Clone)]
struct ConfiguredDeployment {
    deployment: Deployment,
    endpoint: Endpoint,
    credential_ref: CredentialRef,
}

pub struct UmerRuntime {
    timeouts: TimeoutPolicy,
    retry: RetryPolicy,
    transport: Arc<dyn HttpTransport>,
    credentials: Arc<InMemoryCredentialStore>,
    /// `GenerateRequest.model`（Deployment ID）→ 配置
    deployments: Mutex<HashMap<String, ConfiguredDeployment>>,
    catalog: Mutex<Option<Catalog>>,
    demo: AtomicBool,
}

impl UmerRuntime {
    fn new_with_transport(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            timeouts: TimeoutPolicy::default(),
            retry: RetryPolicy::default(),
            transport,
            credentials: Arc::new(InMemoryCredentialStore::new()),
            deployments: Mutex::new(HashMap::new()),
            catalog: Mutex::new(None),
            demo: AtomicBool::new(false),
        }
    }
}

pub struct UmerStream {
    rx: Receiver<SequencedEvent>,
    cancel: CancelToken,
    worker: Option<JoinHandle<()>>,
    closed: bool,
}

/// C 侧数据载体。`json` 为 UTF-8，调用方必须用 `runtime_string_free` 释放。
///
/// `runtime_stream_next` 用它交付事件；`runtime_status` 用它交付运行时状态。
#[repr(C)]
pub struct CUmerEvent {
    pub sequence: u64,
    pub json: *mut c_char,
    pub json_len: usize,
}

/// 把 JSON 交给调用方（所有权转移）。
///
/// # Safety
/// `out` 必须是可写的 `CUmerEvent`。
unsafe fn deliver_json(out: *mut CUmerEvent, sequence: u64, json: String) -> i32 {
    match CString::new(json) {
        Ok(c) => {
            let len = c.as_bytes().len();
            unsafe {
                (*out).sequence = sequence;
                (*out).json = c.into_raw();
                (*out).json_len = len;
            }
            UMER_EVENT
        }
        Err(_) => UMER_ERR_INTERNAL,
    }
}

// ---------- 部署配置 ----------

/// `runtime_set_deployment` 的 JSON 形状：
///
/// ```json
/// {
///   "id": "deepseek/official/openai_chat/deepseek-chat",
///   "provider_id": "deepseek",
///   "protocol": "openai_chat",
///   "endpoint_url": "https://api.deepseek.com/v1",
///   "model_id": "deepseek-chat",
///   "credential_ref": "deepseek/api_key"
/// }
/// ```
///
/// `id` 必须与请求里的 `model` 完全一致；`protocol` 取
/// `openai_chat` / `openai_responses` / `anthropic_messages` / `gemini`
/// / `provider_native`。
#[derive(Debug, Clone, serde::Deserialize)]
struct DeploymentConfig {
    id: String,
    #[serde(default)]
    provider_id: String,
    protocol: ProtocolKind,
    endpoint_url: String,
    #[serde(default)]
    endpoint_id: Option<String>,
    model_id: String,
    #[serde(default = "default_credential_ref")]
    credential_ref: String,
}

fn default_credential_ref() -> String {
    "default/api_key".to_string()
}

/// 按协议选择 Adapter（总案 §51：Adapter 极小，差异不外泄）。
fn adapter_for(
    protocol: ProtocolKind,
    transport: Arc<dyn HttpTransport>,
) -> Arc<dyn ProviderAdapter> {
    match protocol {
        ProtocolKind::OpenAiResponses => Arc::new(OpenAiResponsesAdapter::new(transport)),
        ProtocolKind::AnthropicMessages => Arc::new(AnthropicAdapter::new(transport)),
        ProtocolKind::Gemini => Arc::new(GeminiAdapter::new(transport)),
        // ProviderNative 暂无独立实现：按 openai_chat 形状处理（与参考宿主一致）
        ProtocolKind::OpenAiChat | ProtocolKind::ProviderNative => {
            Arc::new(OpenAiChatAdapter::new(transport))
        }
    }
}

/// Catalog 加载：同时接受裸 Canonical Catalog 与 `model-data build` 的
/// 构建产物（`{"catalog": {...}, "records": [...]}`）。
fn parse_catalog(text: &str) -> Result<Catalog, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let inner = value.get("catalog").cloned().unwrap_or(value);
    let catalog: Catalog = serde_json::from_value(inner).map_err(|e| e.to_string())?;
    catalog.check_format_version().map_err(|e| e.to_string())?;
    Ok(catalog)
}

// ---------- 事件源 ----------

/// 真实链路的事件源工厂：委托给协议 Adapter（重试时由 Engine 重新 open）。
struct AdapterFactory {
    adapter: Arc<dyn ProviderAdapter>,
    request: GenerateRequest,
    endpoint: Endpoint,
    deployment: Deployment,
    credentials: Arc<InMemoryCredentialStore>,
    credential_ref: CredentialRef,
}

impl SourceFactory for AdapterFactory {
    fn open(&self) -> Result<Box<dyn ProviderStream>, ModelError> {
        self.adapter.execute(
            &self.request,
            &self.endpoint,
            &self.deployment,
            self.credentials.as_ref(),
            &self.credential_ref,
        )
    }
}

struct DemoStream {
    steps: std::vec::IntoIter<ModelEvent>,
    pulls: usize,
}

/// Demo 节流：首个 Provider 事件延迟 300ms（供 C 宿主确定性地验证
/// WOULD_BLOCK 分支），其余 40ms。Started 由 Engine 即时合成，不在此列。
impl ProviderStream for DemoStream {
    fn next_event(&mut self, deadline: Instant) -> Result<Option<ModelEvent>, ModelError> {
        let Some(event) = self.steps.next() else {
            return Ok(None);
        };
        let throttle = if self.pulls == 0 {
            Duration::from_millis(300)
        } else {
            Duration::from_millis(40)
        };
        self.pulls += 1;
        std::thread::sleep(throttle.min(deadline.saturating_duration_since(Instant::now())));
        Ok(Some(event))
    }
}

struct DemoFactory;

impl SourceFactory for DemoFactory {
    fn open(&self) -> Result<Box<dyn ProviderStream>, ModelError> {
        let text_id = BlockId::from("demo-text");
        let steps = vec![
            ModelEvent::TextStarted {
                block_id: text_id.clone(),
            },
            ModelEvent::TextDelta {
                block_id: text_id.clone(),
                delta: "Hello from ".into(),
            },
            ModelEvent::TextDelta {
                block_id: text_id.clone(),
                delta: "Umber demo".into(),
            },
            ModelEvent::TextEnded {
                block_id: text_id.clone(),
            },
            ModelEvent::UsageUpdated {
                usage: Usage::new(12, 9, 0, 0),
            },
            ModelEvent::Completed {
                response: Box::new(GenerateResponse {
                    invocation_id: InvocationId::from("demo"),
                    content: vec![
                        umber_core::ContentBlock::Text(TextBlock::new("Hello from Umber demo")),
                        umber_core::ContentBlock::ToolCall(ToolCallBlock {
                            call_id: CallId::from("call-1"),
                            name: "noop".into(),
                            arguments_json: "{}".into(),
                        }),
                    ],
                    stop_reason: StopReason::ToolUse,
                    usage: Usage::new(12, 9, 0, 0),
                    provider_context: ProviderContext::default(),
                }),
            },
        ];
        Ok(Box::new(DemoStream {
            steps: steps.into_iter(),
            pulls: 0,
        }))
    }
}

/// 本次 Invocation 走哪条路。
enum Plan {
    Demo,
    Real {
        adapter: Arc<dyn ProviderAdapter>,
        endpoint: Endpoint,
        deployment: Deployment,
        credential_ref: CredentialRef,
        credentials: Arc<InMemoryCredentialStore>,
    },
}

// ---------- C ABI ----------

/// ABI 版本：(major << 16) | minor。宿主必须校验 major 一致（总案 §50.3）。
#[no_mangle]
pub extern "C" fn runtime_abi_version() -> u32 {
    (RUNTIME_ABI_MAJOR << 16) | RUNTIME_ABI_MINOR
}

/// 创建 Runtime 实例。返回 NULL 表示失败。
///
/// 新实例**没有**任何已配置的 Deployment，demo 关闭：
/// 必须先 `runtime_set_deployment` 或 `runtime_set_demo`。
///
/// # Safety
///
/// 返回的句柄必须且只能通过 `runtime_shutdown` 释放一次。
/// 句柄可被任意线程使用（内部状态加锁）。
#[no_mangle]
pub unsafe extern "C" fn runtime_init() -> *mut UmerRuntime {
    let result = catch_unwind(|| {
        let transport: Arc<dyn HttpTransport> =
            Arc::new(RealHttpTransport::new(HttpConfig::default()));
        Box::into_raw(Box::new(UmerRuntime::new_with_transport(transport)))
    });
    result.unwrap_or(std::ptr::null_mut())
}

/// 销毁 Runtime 实例。NULL 安全。
///
/// # Safety
///
/// `rt` 必须是 `runtime_init` 返回且未销毁过的句柄；销毁后不得再使用
/// （包括传给本函数）。不得与仍在使用的流句柄并发调用。
#[no_mangle]
pub unsafe extern "C" fn runtime_shutdown(rt: *mut UmerRuntime) {
    if !rt.is_null() {
        unsafe {
            drop(Box::from_raw(rt));
        }
    }
}

/// 开启 / 关闭内置 demo 事件源（默认关闭）。
///
/// demo 流是**假数据**，只用于验证 ABI 形态，不连接任何 Provider。
/// 未开启且未配置部署时 `runtime_stream_open` 返回
/// `UMER_ERR_NOT_CONFIGURED`。
///
/// # Safety
///
/// `rt` 必须是有效的 Runtime 句柄。线程安全。
#[no_mangle]
pub unsafe extern "C" fn runtime_set_demo(rt: *mut UmerRuntime, enabled: i32) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if rt.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        let runtime = unsafe { &*rt };
        runtime.demo.store(enabled != 0, Ordering::SeqCst);
        UMER_OK
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 注册（或覆盖）一个 Deployment。JSON 形状见本文件 `DeploymentConfig` 文档。
///
/// 同一个 `id` 重复调用即覆盖。成功返回 `UMER_OK`；
/// JSON 不合法或 `len` 与 C 字符串长度不一致返回 `UMER_ERR_OPEN_FAILED`。
///
/// # Safety
///
/// `rt` 必须是有效的 Runtime 句柄；`config_json` 必须指向至少 `len`
/// 字节可读的 UTF-8 内存。线程安全。
#[no_mangle]
pub unsafe extern "C" fn runtime_set_deployment(
    rt: *mut UmerRuntime,
    config_json: *const c_char,
    len: usize,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if rt.is_null() || config_json.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        let bytes = unsafe { CStr::from_ptr(config_json) }.to_bytes();
        if bytes.len() != len {
            return UMER_ERR_OPEN_FAILED;
        }
        let config: DeploymentConfig = match serde_json::from_slice(bytes) {
            Ok(c) => c,
            Err(_) => return UMER_ERR_OPEN_FAILED,
        };
        let runtime = unsafe { &*rt };

        let provider_id = if config.provider_id.trim().is_empty() {
            "custom".to_string()
        } else {
            config.provider_id
        };
        let endpoint = Endpoint {
            id: config.endpoint_id.unwrap_or_else(|| "default".to_string()),
            provider_id,
            url: config.endpoint_url,
        };
        let deployment = Deployment {
            id: DeploymentId::from(config.id.clone()),
            endpoint_id: endpoint.id.clone(),
            protocol: config.protocol,
            model_id: config.model_id,
        };
        let Ok(mut map) = runtime.deployments.lock() else {
            return UMER_ERR_INTERNAL;
        };
        map.insert(
            config.id,
            ConfiguredDeployment {
                deployment,
                endpoint,
                credential_ref: CredentialRef::from(config.credential_ref),
            },
        );
        UMER_OK
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 写入一条凭据到 Runtime 的内存凭据存储。
///
/// **Runtime 不持久化密钥**：进程退出即丢失。需要持久化的宿主应自己落到
/// 系统钥匙串（见 `docs/HOST_INTEGRATION.md` §4），启动时再写入本函数。
/// 密钥绝不进入日志、Catalog 或 Canonical Request。
///
/// # Safety
///
/// `rt` 必须是有效的 Runtime 句柄；`reference` / `secret` 必须是以 NUL
/// 结尾的 UTF-8 C 字符串。线程安全。
#[no_mangle]
pub unsafe extern "C" fn runtime_set_credential(
    rt: *mut UmerRuntime,
    reference: *const c_char,
    secret: *const c_char,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if rt.is_null() || reference.is_null() || secret.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        let reference = unsafe { CStr::from_ptr(reference) }
            .to_string_lossy()
            .into_owned();
        let secret = unsafe { CStr::from_ptr(secret) }
            .to_string_lossy()
            .into_owned();
        let runtime = unsafe { &*rt };
        match runtime
            .credentials
            .set(&CredentialRef::from(reference), SecretString::new(secret))
        {
            Ok(()) => UMER_OK,
            Err(_) => UMER_ERR_INTERNAL,
        }
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 加载 Canonical Catalog（模型知识：能力 / 上下文 / 价格）。
///
/// 接受裸 Catalog 或 `model-data build` 的构建产物；
/// `format_version` 不在兼容区间内即拒绝。
/// **完全离线可用**——Catalog 是随包分发物，不是运行时网络依赖。
///
/// # Safety
///
/// `rt` 必须是有效的 Runtime 句柄；`path` 必须是以 NUL 结尾的 UTF-8
/// C 字符串路径。线程安全。
#[no_mangle]
pub unsafe extern "C" fn runtime_load_catalog(rt: *mut UmerRuntime, path: *const c_char) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if rt.is_null() || path.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        let path = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(p) => p.to_string(),
            Err(_) => return UMER_ERR_CATALOG,
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return UMER_ERR_CATALOG,
        };
        let catalog = match parse_catalog(&text) {
            Ok(c) => c,
            Err(_) => return UMER_ERR_CATALOG,
        };
        let runtime = unsafe { &*rt };
        let Ok(mut slot) = runtime.catalog.lock() else {
            return UMER_ERR_INTERNAL;
        };
        *slot = Some(catalog);
        UMER_OK
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 读取运行时状态（JSON），用于宿主自检与排查。
///
/// 形状：
///
/// ```json
/// {"abi":"0.2","deployments":1,"catalog_loaded":true,"catalog_entries":3230,
///  "demo_fallback":false,"configured_models":["dep-1"]}
/// ```
///
/// 交付的 JSON 归调用方所有，必须用 `runtime_string_free` 释放；
/// 成功返回 `UMER_EVENT`。
///
/// # Safety
///
/// `rt` 必须是有效的 Runtime 句柄；`out` 必须指向可写的 `CUmerEvent`。
/// 线程安全。
#[no_mangle]
pub unsafe extern "C" fn runtime_status(rt: *mut UmerRuntime, out: *mut CUmerEvent) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if rt.is_null() || out.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        unsafe { (*out).json = std::ptr::null_mut() };
        let runtime = unsafe { &*rt };
        let mut models: Vec<String> = match runtime.deployments.lock() {
            Ok(map) => {
                let mut ids: Vec<String> = map.keys().cloned().collect();
                ids.sort();
                ids
            }
            Err(_) => return UMER_ERR_INTERNAL,
        };
        let (catalog_loaded, catalog_entries) = match runtime.catalog.lock() {
            Ok(slot) => match slot.as_ref() {
                Some(catalog) => (true, catalog.entries.len()),
                None => (false, 0usize),
            },
            Err(_) => return UMER_ERR_INTERNAL,
        };
        models.truncate(64);
        let status = serde_json::json!({
            "abi": format!("{RUNTIME_ABI_MAJOR}.{RUNTIME_ABI_MINOR}"),
            "demo_fallback": runtime.demo.load(Ordering::SeqCst),
            "catalog_loaded": catalog_loaded,
            "catalog_entries": catalog_entries,
            "configured_models": models,
        });
        let json = match serde_json::to_string(&status) {
            Ok(j) => j,
            Err(_) => return UMER_ERR_INTERNAL,
        };
        unsafe { deliver_json(out, 0, json) }
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 打开一次 Invocation 流。
///
/// `request_json` 是 Canonical GenerateRequest 的 UTF-8 JSON；
/// 形态校验失败返回 `UMER_ERR_OPEN_FAILED`。
///
/// 路由（0.2）：`request.model` 命中已注册的 Deployment → 真实协议链路；
/// 否则若 `runtime_set_demo(rt, 1)` → demo 流；否则
/// `UMER_ERR_NOT_CONFIGURED`。
///
/// # Safety
///
/// - `rt` 必须是有效的 Runtime 句柄；`request_json` 必须指向至少 `len`
///   字节可读内存；`out` 必须指向可写指针。
/// - 成功时 `*out` 收到新句柄，须用 `runtime_stream_close` 释放恰好一次。
/// - 线程安全：可从任意线程调用；每个流句柄同一时刻只应被一个线程拉取。
#[no_mangle]
pub unsafe extern "C" fn runtime_stream_open(
    rt: *mut UmerRuntime,
    request_json: *const c_char,
    len: usize,
    out: *mut *mut UmerStream,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if rt.is_null() || request_json.is_null() || out.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        let bytes = unsafe { CStr::from_ptr(request_json) }.to_bytes();
        if bytes.len() != len {
            return UMER_ERR_OPEN_FAILED; // len 必须与 C string 内容一致
        }
        let request: GenerateRequest = match serde_json::from_slice(bytes) {
            Ok(r) => r,
            Err(_) => return UMER_ERR_OPEN_FAILED,
        };
        let runtime = unsafe { &*rt };

        let plan = match runtime.deployments.lock() {
            Ok(map) => map.get(&request.model.0).cloned(),
            Err(_) => return UMER_ERR_INTERNAL,
        };
        let plan = match plan {
            Some(configured) => Plan::Real {
                adapter: adapter_for(configured.deployment.protocol, runtime.transport.clone()),
                endpoint: configured.endpoint,
                deployment: configured.deployment,
                credential_ref: configured.credential_ref,
                credentials: runtime.credentials.clone(),
            },
            // 未配置：只有宿主显式开启 demo 才给假流，绝不静默兜底
            None if runtime.demo.load(Ordering::SeqCst) => Plan::Demo,
            None => return UMER_ERR_NOT_CONFIGURED,
        };

        let (tx, rx) = mpsc::channel::<SequencedEvent>();
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        let timeouts = runtime.timeouts.clone();
        let retry = runtime.retry.clone();

        let worker = match std::thread::Builder::new()
            .name("umer-invocation".into())
            .spawn(move || {
                let _ = catch_unwind(AssertUnwindSafe(|| match plan {
                    Plan::Demo => {
                        run_invocation(
                            &DemoFactory,
                            &request,
                            &worker_cancel,
                            &timeouts,
                            &retry,
                            &mut |event| {
                                let _ = tx.send(event);
                            },
                        );
                    }
                    Plan::Real {
                        adapter,
                        endpoint,
                        deployment,
                        credential_ref,
                        credentials,
                    } => {
                        let factory = AdapterFactory {
                            adapter,
                            request: request.clone(),
                            endpoint,
                            deployment,
                            credentials,
                            credential_ref,
                        };
                        run_invocation(
                            &factory,
                            &request,
                            &worker_cancel,
                            &timeouts,
                            &retry,
                            &mut |event| {
                                let _ = tx.send(event);
                            },
                        );
                    }
                }));
            }) {
            Ok(handle) => handle,
            Err(_) => return UMER_ERR_INTERNAL,
        };

        unsafe {
            *out = Box::into_raw(Box::new(UmerStream {
                rx,
                cancel,
                worker: Some(worker),
                closed: false,
            }));
        }
        UMER_OK
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 拉取下一个事件（阻塞至多 `timeout_ms`）。
///
/// 事件 JSON 的所有权转移到调用方，用 `runtime_string_free` 释放；
/// `UMER_WOULD_BLOCK` 表示超时但流未关闭，可继续拉取。
///
/// # Safety
///
/// - `stream` 必须是 `runtime_stream_open` 产出且未关闭的句柄。
/// - `out` 必须指向可写的 `CUmerEvent`；成功时其 `json` 字段由本函数
///   分配，调用方必须用 `runtime_string_free` 释放恰好一次。
/// - 线程安全：同一流句柄不得被两个线程同时拉取。
#[no_mangle]
pub unsafe extern "C" fn runtime_stream_next(
    stream: *mut UmerStream,
    timeout_ms: u32,
    out: *mut CUmerEvent,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if stream.is_null() || out.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        unsafe { (*out).json = std::ptr::null_mut() };
        let stream = unsafe { &mut *stream };
        if stream.closed {
            return UMER_CLOSED;
        }
        match stream
            .rx
            .recv_timeout(Duration::from_millis(timeout_ms as u64))
        {
            Ok(event) => {
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(_) => return UMER_ERR_INTERNAL,
                };
                let sequence = event.sequence;
                if event.event.is_terminal() {
                    stream.closed = true;
                }
                unsafe { deliver_json(out, sequence, json) }
            }
            Err(RecvTimeoutError::Timeout) => UMER_WOULD_BLOCK,
            Err(RecvTimeoutError::Disconnected) => {
                stream.closed = true;
                UMER_CLOSED
            }
        }
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 请求取消。取消语义由 Runtime Invocation 承载（总案 §25）；
/// 已关闭的流返回 OK。
///
/// # Safety
///
/// `stream` 必须是有效或 NULL 的流句柄。线程安全：可与
/// `runtime_stream_next` 并发调用（这是取消期间的标准用法）。
#[no_mangle]
pub unsafe extern "C" fn runtime_stream_cancel(stream: *mut UmerStream) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if stream.is_null() {
            return UMER_ERR_NULL_ARGUMENT;
        }
        let stream = unsafe { &*stream };
        stream.cancel.cancel();
        UMER_OK
    }));
    result.unwrap_or(UMER_ERR_INTERNAL)
}

/// 关闭并回收流。NULL 安全；关闭后流句柄不可再用。
///
/// # Safety
///
/// `stream` 必须是 `runtime_stream_open` 产出且未被关闭过的句柄，
/// 或为 NULL。关闭会等待内部 worker 结束；调用方必须保证没有其他线程
/// 仍在本流句柄上阻塞于 `runtime_stream_next`。
#[no_mangle]
pub unsafe extern "C" fn runtime_stream_close(stream: *mut UmerStream) {
    if !stream.is_null() {
        let stream = unsafe { Box::from_raw(stream) };
        stream.cancel.cancel();
        if let Some(worker) = stream.worker {
            let _ = worker.join();
        }
    }
}

/// 释放本 ABI 交付的 JSON（`runtime_stream_next` / `runtime_status`）。
/// NULL 安全。
///
/// # Safety
///
/// `ptr` 必须是本 ABI 分配且尚未释放的指针，或为 NULL。
/// 不得释放任何其他来源的指针。
#[no_mangle]
pub unsafe extern "C" fn runtime_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umber_core::message::Message;
    use umber_protocol::ScriptedTransport;

    fn request_json() -> String {
        let req = GenerateRequest::new("demo", vec![Message::user("hi")]);
        serde_json::to_string(&req).unwrap()
    }

    fn cstring(value: &str) -> CString {
        CString::new(value).unwrap()
    }

    /// 打开一个带脚本化传输的 Runtime（跳过 `runtime_init`，测试内部接线）。
    fn scripted_runtime(sse: &str) -> (*mut UmerRuntime, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new());
        transport.enqueue_sse(200, sse);
        let rt = Box::into_raw(Box::new(UmerRuntime::new_with_transport(transport.clone())));
        (rt, transport)
    }

    fn open_stream(rt: *mut UmerRuntime, request_json: &str) -> Result<*mut UmerStream, i32> {
        let json = cstring(request_json);
        let mut stream: *mut UmerStream = std::ptr::null_mut();
        let status =
            unsafe { runtime_stream_open(rt, json.as_ptr(), json.as_bytes().len(), &mut stream) };
        if status == UMER_OK {
            Ok(stream)
        } else {
            Err(status)
        }
    }

    /// 拉完整条流，返回 (事件 JSON 列表, 事件类型列表)。
    fn drain(stream: *mut UmerStream) -> (Vec<String>, Vec<String>) {
        let mut jsons = Vec::new();
        let mut kinds = Vec::new();
        loop {
            let mut ev = CUmerEvent {
                sequence: 0,
                json: std::ptr::null_mut(),
                json_len: 0,
            };
            let status = unsafe { runtime_stream_next(stream, 5_000, &mut ev) };
            match status {
                UMER_EVENT => {
                    let text = unsafe { CStr::from_ptr(ev.json).to_string_lossy().into_owned() };
                    unsafe { runtime_string_free(ev.json) };
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                    kinds.push(value["event"]["type"].as_str().unwrap_or("?").to_string());
                    jsons.push(text);
                }
                UMER_WOULD_BLOCK => continue,
                UMER_CLOSED => break,
                other => panic!("unexpected status {other}"),
            }
        }
        (jsons, kinds)
    }

    #[test]
    fn abi_version_is_major_zero_minor_two() {
        let v = runtime_abi_version();
        assert_eq!(v >> 16, RUNTIME_ABI_MAJOR);
        assert_eq!(v & 0xFFFF, 2);
    }

    #[test]
    fn runtime_handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UmerRuntime>();
    }

    #[test]
    fn null_arguments_are_rejected_not_crash() {
        unsafe {
            assert_eq!(
                runtime_stream_next(std::ptr::null_mut(), 1, std::ptr::null_mut()),
                UMER_ERR_NULL_ARGUMENT
            );
            assert_eq!(
                runtime_stream_cancel(std::ptr::null_mut()),
                UMER_ERR_NULL_ARGUMENT
            );
            assert_eq!(
                runtime_stream_open(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut()
                ),
                UMER_ERR_NULL_ARGUMENT
            );
            assert_eq!(
                runtime_set_deployment(std::ptr::null_mut(), std::ptr::null(), 0),
                UMER_ERR_NULL_ARGUMENT
            );
            assert_eq!(
                runtime_set_credential(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()),
                UMER_ERR_NULL_ARGUMENT
            );
            assert_eq!(
                runtime_load_catalog(std::ptr::null_mut(), std::ptr::null()),
                UMER_ERR_NULL_ARGUMENT
            );
            assert_eq!(
                runtime_status(std::ptr::null_mut(), std::ptr::null_mut()),
                UMER_ERR_NULL_ARGUMENT
            );
        }
    }

    /// 关键行为变更：未配置 + 未开 demo（默认）→ 明确报错，绝不静默返回假数据。
    #[test]
    fn unconfigured_request_is_rejected_not_silently_demoed() {
        let rt = unsafe { runtime_init() };
        assert!(!rt.is_null());
        let err = open_stream(rt, &request_json()).unwrap_err();
        assert_eq!(err, UMER_ERR_NOT_CONFIGURED);
        unsafe { runtime_shutdown(rt) };
    }

    #[test]
    fn demo_flag_enables_the_abi_spike_stream() {
        let rt = unsafe { runtime_init() };
        unsafe { assert_eq!(runtime_set_demo(rt, 1), UMER_OK) };
        let stream = open_stream(rt, &request_json()).unwrap();
        let (jsons, kinds) = drain(stream);
        unsafe {
            runtime_stream_close(stream);
            runtime_shutdown(rt);
        }
        assert_eq!(kinds.first().map(String::as_str), Some("started"));
        assert_eq!(
            kinds
                .iter()
                .filter(|k| matches!(k.as_str(), "completed" | "failed" | "cancelled"))
                .count(),
            1,
            "exactly one terminal event"
        );
        assert!(jsons.iter().any(|j| j.contains("Umber demo")));
    }

    /// 端到端：C ABI → 真实 Adapter → ScriptedTransport → Engine → 事件。
    ///
    /// 这条测试是"ABI 真接线"的证据：请求体由 openai_chat Adapter 构造、
    /// 凭据真的送进了 Authorization 头、SSE 被解析成 Canonical 事件。
    #[test]
    fn configured_deployment_runs_through_the_real_adapter() {
        let sse = {
            let c1 = serde_json::json!({"choices":[{"index":0,"delta":{"content":"你"},"finish_reason":null}]});
            let c2 = serde_json::json!({"choices":[{"index":0,"delta":{"content":"好"},"finish_reason":null}]});
            let c3 = serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
            let u = serde_json::json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}});
            format!("data: {c1}\n\ndata: {c2}\n\ndata: {c3}\n\ndata: {u}\n\ndata: [DONE]\n\n")
        };
        let (rt, transport) = scripted_runtime(&sse);

        let config = serde_json::json!({
            "id": "deepseek/official/openai_chat/deepseek-chat",
            "provider_id": "deepseek",
            "protocol": "openai_chat",
            "endpoint_url": "https://api.deepseek.com/v1",
            "model_id": "deepseek-chat",
            "credential_ref": "deepseek/api_key"
        })
        .to_string();
        let cfg = cstring(&config);
        unsafe {
            assert_eq!(
                runtime_set_deployment(rt, cfg.as_ptr(), cfg.as_bytes().len()),
                UMER_OK
            );
        }
        let reference = cstring("deepseek/api_key");
        let secret = cstring("sk-test-only");
        unsafe {
            assert_eq!(
                runtime_set_credential(rt, reference.as_ptr(), secret.as_ptr()),
                UMER_OK
            );
        }

        let request = GenerateRequest::new(
            "deepseek/official/openai_chat/deepseek-chat",
            vec![Message::user("hi")],
        );
        let request_json = serde_json::to_string(&request).unwrap();
        let stream = open_stream(rt, &request_json).unwrap();
        let (jsons, kinds) = drain(stream);
        unsafe {
            runtime_stream_close(stream);
            runtime_shutdown(rt);
        }

        // 事件来自真实 SSE，而不是 demo 的固定文本
        let mut text = String::new();
        for json in &jsons {
            let value: serde_json::Value = serde_json::from_str(json).unwrap();
            if value["event"]["type"] == "text_delta" {
                text.push_str(value["event"]["delta"].as_str().unwrap_or(""));
            }
        }
        assert_eq!(text, "你好");
        assert_eq!(
            kinds
                .iter()
                .filter(|k| matches!(k.as_str(), "completed" | "failed" | "cancelled"))
                .count(),
            1
        );
        assert!(kinds.contains(&"completed".to_string()));

        // 请求确实由 Adapter 按 openai_chat 形状构造，且凭据送到了 header
        let last = transport
            .last_request()
            .expect("adapter must issue a request");
        assert_eq!(last.url, "https://api.deepseek.com/v1/chat/completions");
        assert!(last
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("authorization")));
        let body: serde_json::Value =
            serde_json::from_str(last.body.as_deref().expect("body")).unwrap();
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn malformed_request_json_is_rejected() {
        let rt = unsafe { runtime_init() };
        let bad = cstring("{\"model\":");
        let mut stream: *mut UmerStream = std::ptr::null_mut();
        let status =
            unsafe { runtime_stream_open(rt, bad.as_ptr(), bad.as_bytes().len(), &mut stream) };
        assert_eq!(status, UMER_ERR_OPEN_FAILED);
        unsafe { runtime_shutdown(rt) };
    }

    #[test]
    fn len_mismatch_is_rejected() {
        let rt = unsafe { runtime_init() };
        let json = cstring(&request_json());
        let mut stream: *mut UmerStream = std::ptr::null_mut();
        let status = unsafe { runtime_stream_open(rt, json.as_ptr(), 3, &mut stream) };
        assert_eq!(status, UMER_ERR_OPEN_FAILED);
        unsafe { runtime_shutdown(rt) };
    }

    #[test]
    fn deployment_config_rejects_malformed_json() {
        let rt = unsafe { runtime_init() };
        let bad = cstring("{\"id\":\"x\"}");
        let status = unsafe { runtime_set_deployment(rt, bad.as_ptr(), bad.as_bytes().len()) };
        assert_eq!(status, UMER_ERR_OPEN_FAILED);
        unsafe { runtime_shutdown(rt) };
    }

    #[test]
    fn catalog_load_rejects_bad_path_and_bad_format() {
        let rt = unsafe { runtime_init() };
        let missing = cstring("Z:/definitely/not/here.json");
        assert_eq!(
            unsafe { runtime_load_catalog(rt, missing.as_ptr()) },
            UMER_ERR_CATALOG
        );

        let dir = std::env::temp_dir().join(format!("umer-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_format.json");
        std::fs::write(
            &path,
            r#"{"format_version":99,"generated_at_unix":0,"identities":[],"entries":[],"deployments":[]}"#,
        )
        .unwrap();
        let p = cstring(path.to_str().unwrap());
        assert_eq!(
            unsafe { runtime_load_catalog(rt, p.as_ptr()) },
            UMER_ERR_CATALOG
        );

        let good = dir.join("good.json");
        std::fs::write(
            &good,
            r#"{"format_version":1,"generated_at_unix":0,"identities":[],"entries":[],"deployments":[]}"#,
        )
        .unwrap();
        let p = cstring(good.to_str().unwrap());
        assert_eq!(unsafe { runtime_load_catalog(rt, p.as_ptr()) }, UMER_OK);

        // status 反映已加载的目录
        let mut ev = CUmerEvent {
            sequence: 0,
            json: std::ptr::null_mut(),
            json_len: 0,
        };
        assert_eq!(unsafe { runtime_status(rt, &mut ev) }, UMER_EVENT);
        let text = unsafe { CStr::from_ptr(ev.json).to_string_lossy().into_owned() };
        unsafe { runtime_string_free(ev.json) };
        assert!(text.contains("\"catalog_loaded\":true"));

        let _ = std::fs::remove_dir_all(&dir);
        unsafe { runtime_shutdown(rt) };
    }
}
