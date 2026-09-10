//! Stable C ABI — 拉取式流（总案 §41.6 §50，ABI spike 实现）。
//!
//! ABI 形态（冻结候选，随 spike 验证）：
//! - 拉取式：`runtime_stream_next` 阻塞等待 + 超时；不向 C 暴露 async/future
//! - 错误码：正数 = 流状态（事件 / 关闭 / 暂无数据），负数 = 错误
//! - 句柄式内存：谁分配谁释放——事件 JSON 由 `runtime_string_free` 释放
//! - panic 不穿越边界：所有入口 catch_unwind → 错误码
//! - 字符串一律 UTF-8 + 显式长度
//! - ABI 版本：major << 16 | minor；宿主握手校验 major
//!
//! 本 spike 的事件源是内置 demo 流（engine + fake 事件），
//! 用于验证 ABI 形态本身；真实协议 Adapter 在 M2+ 接入同一形态。

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use runtime_core::content::{TextBlock, ToolCallBlock};
use runtime_core::error::ModelError;
use runtime_core::event::{ModelEvent, SequencedEvent};
use runtime_core::ids::{BlockId, CallId, InvocationId};
use runtime_core::request::GenerateRequest;
use runtime_core::response::{GenerateResponse, ProviderContext, StopReason};
use runtime_core::usage::Usage;
use runtime_engine::{
    run_invocation, CancelToken, ProviderStream, RetryPolicy, SourceFactory, TimeoutPolicy,
};

// ---------- 常量契约 ----------

pub const RUNTIME_ABI_MAJOR: u32 = 0;
pub const RUNTIME_ABI_MINOR: u32 = 1;

pub const UMER_OK: i32 = 0;
pub const UMER_ERR_NULL_ARGUMENT: i32 = -1;
pub const UMER_ERR_ABI_MISMATCH: i32 = -2;
pub const UMER_ERR_OPEN_FAILED: i32 = -3;
pub const UMER_ERR_INTERNAL: i32 = -4;

/// `runtime_stream_next` 状态：事件已交付。
pub const UMER_EVENT: i32 = 1;
/// 流已终结（终结合成事件已在之前交付）。
pub const UMER_CLOSED: i32 = 2;
/// 等待超时，流可能仍有后续事件。
pub const UMER_WOULD_BLOCK: i32 = 3;

// ---------- Opaque 句柄 ----------

pub struct UmerRuntime {
    timeouts: TimeoutPolicy,
    retry: RetryPolicy,
}

pub struct UmerStream {
    rx: Receiver<SequencedEvent>,
    cancel: CancelToken,
    worker: Option<JoinHandle<()>>,
    closed: bool,
}

/// C 侧事件结构。`json` 为 UTF-8，调用方必须用 `runtime_string_free` 释放。
#[repr(C)]
pub struct CUmerEvent {
    pub sequence: u64,
    pub json: *mut c_char,
    pub json_len: usize,
}

// ---------- 内部 demo 事件源 ----------

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
                delta: "UMR ABI spike".into(),
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
                        runtime_core::ContentBlock::Text(TextBlock::new(
                            "Hello from UMR ABI spike",
                        )),
                        runtime_core::ContentBlock::ToolCall(ToolCallBlock {
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

// ---------- C ABI ----------

/// ABI 版本：(major << 16) | minor。宿主必须校验 major 一致（总案 §50.3）。
#[no_mangle]
pub extern "C" fn runtime_abi_version() -> u32 {
    (RUNTIME_ABI_MAJOR << 16) | RUNTIME_ABI_MINOR
}

/// 创建 Runtime 实例。返回 NULL 表示失败。
///
/// # Safety
///
/// 返回的句柄必须且只能通过 `runtime_shutdown` 释放一次。
/// 句柄可被任意线程使用；`UmerRuntime` 本身不暴露字段。
#[no_mangle]
pub unsafe extern "C" fn runtime_init() -> *mut UmerRuntime {
    let result = catch_unwind(|| {
        Box::into_raw(Box::new(UmerRuntime {
            timeouts: TimeoutPolicy::default(),
            retry: RetryPolicy::default(),
        }))
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

/// 打开一次 Invocation 流。
///
/// `request_json` 是 Canonical GenerateRequest 的 UTF-8 JSON；
/// 形态校验失败返回 `UMER_ERR_OPEN_FAILED`。
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

        let (tx, rx) = mpsc::channel::<SequencedEvent>();
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        let timeouts = runtime.timeouts.clone();
        let retry = runtime.retry.clone();

        let worker = match std::thread::Builder::new()
            .name("umer-invocation".into())
            .spawn(move || {
                let _ = catch_unwind(AssertUnwindSafe(|| {
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
            Ok(event) => match serde_json::to_string(&event) {
                Ok(json) => match CString::new(json) {
                    Ok(c) => {
                        let len = c.as_bytes().len();
                        unsafe {
                            (*out).sequence = event.sequence;
                            (*out).json = c.into_raw();
                            (*out).json_len = len;
                        }
                        if event.event.is_terminal() {
                            stream.closed = true;
                        }
                        UMER_EVENT
                    }
                    Err(_) => UMER_ERR_INTERNAL,
                },
                Err(_) => UMER_ERR_INTERNAL,
            },
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

/// 释放 `runtime_stream_next` 交付的事件 JSON。NULL 安全。
///
/// # Safety
///
/// `ptr` 必须是 `runtime_string_free` 尚未释放过的、由本 ABI 分配的
/// 指针，或为 NULL。不得释放任何其他来源的指针。
#[no_mangle]
pub unsafe extern "C" fn runtime_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::message::Message;

    fn request_json() -> String {
        let req = GenerateRequest::new("demo", vec![Message::user("hi")]);
        serde_json::to_string(&req).unwrap()
    }

    #[test]
    fn abi_major_is_stable_zero_for_spike() {
        let v = runtime_abi_version();
        assert_eq!(v >> 16, RUNTIME_ABI_MAJOR);
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
        }
    }

    #[test]
    fn full_pull_loop_reaches_exactly_one_terminal() {
        let rt = unsafe { runtime_init() };
        assert!(!rt.is_null());
        let json = CString::new(request_json()).unwrap();
        let mut stream: *mut UmerStream = std::ptr::null_mut();
        let status =
            unsafe { runtime_stream_open(rt, json.as_ptr(), json.as_bytes().len(), &mut stream) };
        assert_eq!(status, UMER_OK);
        assert!(!stream.is_null());

        let mut terminals = 0;
        let mut sequences = Vec::new();
        loop {
            let mut ev = CUmerEvent {
                sequence: 0,
                json: std::ptr::null_mut(),
                json_len: 0,
            };
            let status = unsafe { runtime_stream_next(stream, 2_000, &mut ev) };
            match status {
                UMER_EVENT => {
                    sequences.push(ev.sequence);
                    let json_text =
                        unsafe { CStr::from_ptr(ev.json).to_string_lossy().into_owned() };
                    unsafe { runtime_string_free(ev.json) };
                    assert!(json_text.contains("\"type\""));
                    if json_text.contains("\"completed\"")
                        || json_text.contains("\"failed\"")
                        || json_text.contains("\"cancelled\"")
                    {
                        terminals += 1;
                    }
                }
                UMER_WOULD_BLOCK => continue,
                UMER_CLOSED => break,
                other => panic!("unexpected status {other}"),
            }
        }
        assert_eq!(terminals, 1, "exactly one terminal event");
        assert_eq!(sequences.first().copied(), Some(0), "Started is sequence 0");
        unsafe {
            runtime_stream_close(stream);
            runtime_shutdown(rt);
        }
    }

    #[test]
    fn malformed_request_json_is_rejected() {
        let rt = unsafe { runtime_init() };
        let bad = CString::new("{\"model\":").unwrap();
        let mut stream: *mut UmerStream = std::ptr::null_mut();
        let status =
            unsafe { runtime_stream_open(rt, bad.as_ptr(), bad.as_bytes().len(), &mut stream) };
        assert_eq!(status, UMER_ERR_OPEN_FAILED);
        unsafe { runtime_shutdown(rt) };
    }

    #[test]
    fn len_mismatch_is_rejected() {
        let rt = unsafe { runtime_init() };
        let json = CString::new(request_json()).unwrap();
        let mut stream: *mut UmerStream = std::ptr::null_mut();
        let status = unsafe { runtime_stream_open(rt, json.as_ptr(), 3, &mut stream) };
        assert_eq!(status, UMER_ERR_OPEN_FAILED);
        unsafe { runtime_shutdown(rt) };
    }
}
