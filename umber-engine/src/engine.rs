//! Invocation 驱动：终结事件保证 + 四段超时 + Retry + 部分结果（总案 §23–§31）。
//!
//! 阻塞式设计：调用方（FFI 桥 / 测试 / 未来异步包装层）把它放到独立线程，
//! 通过 `on_event` 回调接收带全局 sequence 的事件流。

use std::thread;
use std::time::{Duration, Instant};

use umber_core::error::{ErrorDetail, ModelError, TimeoutKind};
use umber_core::event::{ModelEvent, SequencedEvent};
use umber_core::ids::InvocationId;
use umber_core::invocation::PartialOutput;
use umber_core::request::GenerateRequest;
use umber_core::response::GenerateResponse;

use crate::assembler::StreamAssembler;
use crate::cancel::CancelToken;
use crate::retry::{decide, RetryDecision, RetryPolicy};
use crate::source::SourceFactory;
use crate::timeouts::TimeoutPolicy;

/// 一次运行的最终结果。
#[derive(Debug, Clone)]
pub struct InvocationOutcome {
    pub invocation_id: InvocationId,
    /// 终结事件为 Completed 时 Some。
    pub response: Option<GenerateResponse>,
    /// 始终可用（总案 §25.1）；Failed / Cancelled 时携带已产出的部分内容。
    pub partial: PartialOutput,
}

struct Emitter<'a> {
    next: u64,
    sink: &'a mut dyn FnMut(SequencedEvent),
}

impl Emitter<'_> {
    fn emit(&mut self, event: ModelEvent) {
        let sequence = self.next;
        self.next += 1;
        (self.sink)(SequencedEvent { sequence, event });
    }
}

static INV_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn new_invocation_id() -> String {
    let n = INV_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("inv-{ms}-{}", n + 1)
}

/// 驱动一次 Invocation 直到终结事件。
pub fn run_invocation(
    factory: &dyn SourceFactory,
    request: &GenerateRequest,
    cancel: &CancelToken,
    timeouts: &TimeoutPolicy,
    retry: &RetryPolicy,
    on_event: &mut dyn FnMut(SequencedEvent),
) -> InvocationOutcome {
    // request 属于 Invocation 记录与 FFI 层展示；事件源工厂已绑定同一请求
    let _ = request;
    let started = Instant::now();
    let invocation_id = InvocationId::from(new_invocation_id());
    let mut emitter = Emitter {
        next: 0,
        sink: on_event,
    };
    let mut assembler = StreamAssembler::new();
    let mut attempts = 0u32;
    let mut first_event_seen = false;

    emitter.emit(ModelEvent::Started {
        invocation_id: invocation_id.clone(),
    });

    'outer: loop {
        if cancel.is_cancelled() {
            return finish_cancelled(&mut emitter, &assembler, invocation_id);
        }
        if total_expired(&started, timeouts) {
            return finish_failed(
                &mut emitter,
                &assembler,
                invocation_id,
                ModelError::Timeout {
                    kind: TimeoutKind::Total,
                    detail: ErrorDetail::new("total timeout expired"),
                },
            );
        }

        // 打开事件源（connect 预算 = min(connect, total 剩余)；阻塞式 open 事后计量）
        let mut src = match factory.open() {
            Ok(s) => s,
            Err(err) => match decide(retry, attempts, &err, false) {
                RetryDecision::Retry { wait } => {
                    attempts += 1;
                    if !sleep_interruptible(cancel, wait) {
                        return finish_cancelled(&mut emitter, &assembler, invocation_id);
                    }
                    continue 'outer;
                }
                RetryDecision::GiveUp => {
                    let err = relabel_timeout(err, TimeoutKind::Connect);
                    return finish_failed(&mut emitter, &assembler, invocation_id, err);
                }
            },
        };

        'stream: loop {
            if cancel.is_cancelled() {
                return finish_cancelled(&mut emitter, &assembler, invocation_id);
            }
            if total_expired(&started, timeouts) {
                return finish_failed(
                    &mut emitter,
                    &assembler,
                    invocation_id,
                    ModelError::Timeout {
                        kind: TimeoutKind::Total,
                        detail: ErrorDetail::new("total timeout expired"),
                    },
                );
            }

            let expected_kind = if first_event_seen {
                TimeoutKind::Idle
            } else {
                TimeoutKind::FirstToken
            };
            let budget = if first_event_seen {
                timeouts.idle
            } else {
                timeouts.first_token
            };
            let deadline = Instant::now() + clamp_to_total(&started, timeouts, budget);

            // 分片拉取：取消响应粒度 = PULL_SLICE；
            // 真实 HTTP Adapter 的取消会直接中断连接，此处保证 fake/慢源同样可取消。
            const PULL_SLICE: Duration = Duration::from_millis(100);
            let pulled = 'pull: loop {
                if cancel.is_cancelled() {
                    break 'pull Err(ModelError::Cancelled);
                }
                let slice_deadline = deadline.min(Instant::now() + PULL_SLICE);
                match src.next_event(slice_deadline) {
                    Err(ModelError::Timeout { .. }) if slice_deadline < deadline => {
                        continue 'pull; // 分片到点，未到真实 deadline，继续等待
                    }
                    other => break 'pull other,
                }
            };

            match pulled {
                Ok(Some(event)) => {
                    // 契约违约：Adapter 不发 Started；忽略之
                    if matches!(event, ModelEvent::Started { .. }) {
                        continue 'stream;
                    }
                    if event.is_terminal() {
                        let response = if let ModelEvent::Completed { response } = &event {
                            // Adapter 合成的 Completed 以 Engine 的 Invocation
                            // 身份为准（总案 §25：invocation_id 归 Runtime 签发）
                            let mut r = response.as_ref().clone();
                            r.invocation_id = invocation_id.clone();
                            Some(r)
                        } else {
                            None
                        };
                        emitter.emit(event);
                        return InvocationOutcome {
                            invocation_id,
                            response,
                            partial: assembler.to_partial(None),
                        };
                    }
                    first_event_seen = true;
                    assembler.absorb(&event);
                    emitter.emit(event);
                }
                Ok(None) => {
                    // EOF 而无终结事件：Runtime 合成 Failed（总案 §23.1）
                    let err = if assembler.seen_content() || assembler.usage().is_some() {
                        ModelError::ProviderError {
                            status: None,
                            detail: ErrorDetail::new("stream ended without terminal event"),
                        }
                    } else {
                        ModelError::EmptyResponse
                    };
                    return finish_failed(&mut emitter, &assembler, invocation_id, err);
                }
                Err(err) => {
                    // 取消优先于一切错误语义
                    if cancel.is_cancelled() {
                        return finish_cancelled(&mut emitter, &assembler, invocation_id);
                    }
                    let err = relabel_timeout(err, expected_kind);
                    match decide(retry, attempts, &err, assembler.seen_content()) {
                        RetryDecision::Retry { wait } => {
                            attempts += 1;
                            if !sleep_interruptible(cancel, wait) {
                                return finish_cancelled(&mut emitter, &assembler, invocation_id);
                            }
                            continue 'outer; // 安全重放：重新打开事件源
                        }
                        RetryDecision::GiveUp => {
                            return finish_failed(&mut emitter, &assembler, invocation_id, err);
                        }
                    }
                }
            }
        }
    }
}

fn total_expired(started: &Instant, timeouts: &TimeoutPolicy) -> bool {
    timeouts.total.is_some_and(|t| started.elapsed() >= t)
}

fn clamp_to_total(started: &Instant, timeouts: &TimeoutPolicy, budget: Duration) -> Duration {
    match timeouts.total {
        Some(total) => {
            let remaining = total.saturating_sub(started.elapsed());
            budget.min(remaining)
        }
        None => budget,
    }
}

/// Engine 是超时策略的唯一真值：来源上报的 Timeout 一律按 Engine 的阶段定义重标。
fn relabel_timeout(err: ModelError, expected: TimeoutKind) -> ModelError {
    match err {
        ModelError::Timeout { detail, .. } => ModelError::Timeout {
            kind: expected,
            detail,
        },
        other => other,
    }
}

/// 分片睡眠，取消随时可中断。返回 false 表示已取消。
fn sleep_interruptible(cancel: &CancelToken, wait: Duration) -> bool {
    const SLICE: Duration = Duration::from_millis(10);
    let mut remaining = wait;
    while remaining > Duration::ZERO {
        if cancel.is_cancelled() {
            return false;
        }
        let step = remaining.min(SLICE);
        thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
    !cancel.is_cancelled()
}

fn finish_failed(
    emitter: &mut Emitter,
    assembler: &StreamAssembler,
    invocation_id: InvocationId,
    err: ModelError,
) -> InvocationOutcome {
    emitter.emit(ModelEvent::Failed { error: err.clone() });
    InvocationOutcome {
        invocation_id,
        response: None,
        partial: assembler.to_partial(Some(err)),
    }
}

fn finish_cancelled(
    emitter: &mut Emitter,
    assembler: &StreamAssembler,
    invocation_id: InvocationId,
) -> InvocationOutcome {
    emitter.emit(ModelEvent::Cancelled);
    InvocationOutcome {
        invocation_id,
        response: None,
        partial: assembler.to_partial(Some(ModelError::Cancelled)),
    }
}
