//! 读线程 → 通道 → 拉取式事件流（总案 §39.4 execute 的产物形态）。
//!
//! Adapter 在独立线程里读传输层 SSE 行、跑协议解析器、把 Canonical 事件
//! 推入通道；`ChannelStream` 在调用线程按 Engine 的 deadline 阻塞拉取。
//! 取消响应由两层保证：
//! - Engine 的 100ms 分片拉取随时停等；
//! - worker 用有界通道 + `try_send` 重试，缓冲写满后仍能在 50ms 内观察到
//!   取消；流被丢弃（接收端析构）后 `try_send` 立刻 Disconnected，worker
//!   随之退出——读阻塞的绝对上限由传输层读超时负责。

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use runtime_core::error::{ErrorDetail, ModelError, TimeoutKind};
use runtime_core::event::ModelEvent;
use runtime_engine::{CancelToken, ProviderStream};

const CHANNEL_CAPACITY: usize = 256;
const SEND_RETRY: Duration = Duration::from_millis(50);

pub struct ChannelStream {
    rx: Receiver<ModelEvent>,
}

impl ChannelStream {
    /// 启动 worker：逐行喂协议解析器，解析出的事件即时入通道。
    ///
    /// `on_line` 返回 false 表示协议要求终止读取（通常在合成终结合同事件后）。
    /// worker 不 join：接收端析构后 `try_send` 返回 Disconnected，worker 自行退出。
    pub fn spawn<F>(
        cancel: CancelToken,
        lines: Box<dyn Iterator<Item = Result<String, ModelError>> + Send>,
        mut on_line: F,
    ) -> Self
    where
        F: FnMut(&str, &mut Vec<ModelEvent>) -> bool + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<ModelEvent>(CHANNEL_CAPACITY);
        let _ = std::thread::Builder::new()
            .name("umer-protocol".into())
            .spawn(move || {
                let mut pending: Vec<ModelEvent> = Vec::new();
                for line in lines {
                    if cancel.is_cancelled() {
                        return;
                    }
                    match line {
                        Ok(l) => {
                            let keep = on_line(&l, &mut pending);
                            // 先冲刷事件（终结事件就在最后一批里），再决定是否退出
                            for e in pending.drain(..) {
                                if !send_aware(&tx, e, &cancel) {
                                    return;
                                }
                            }
                            if !keep {
                                return;
                            }
                        }
                        Err(_) => return, // 读失败 → EOF 语义，Engine 合成 Failed
                    }
                }
            });
        Self { rx }
    }
}

/// 有界发送：缓冲满时以 50ms 步长重试，期间可被取消打断。
fn send_aware(tx: &SyncSender<ModelEvent>, event: ModelEvent, cancel: &CancelToken) -> bool {
    let mut e = event;
    loop {
        if cancel.is_cancelled() {
            return false;
        }
        match tx.try_send(e) {
            Ok(()) => return true,
            Err(TrySendError::Full(again)) => {
                e = again;
                std::thread::sleep(SEND_RETRY);
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

impl ProviderStream for ChannelStream {
    fn next_event(&mut self, deadline: Instant) -> Result<Option<ModelEvent>, ModelError> {
        match self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(event) => Ok(Some(event)),
            Err(RecvTimeoutError::Timeout) => Err(ModelError::Timeout {
                kind: TimeoutKind::Idle, // Engine 按阶段重标（总案 §31.1）
                detail: ErrorDetail::new("upstream read exceeded deadline"),
            }),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }
}
