//! 可脚本化 fake provider（总案 §52）。
//!
//! 支持注入：正常事件、延迟（触发四段超时）、错误（可重试 / 不可重试）、
//! 正常 EOF（触发终结事件合成）、open 阶段失败（触发 Retry 重开）。

use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

use umber_core::error::{ErrorDetail, ModelError, TimeoutKind};
use umber_core::event::ModelEvent;

use umber_engine::{ProviderStream, SourceFactory};

/// 脚本步骤。
#[derive(Debug, Clone)]
pub enum Step {
    Emit(ModelEvent),
    /// 睡眠；若会超过 deadline，则睡到 deadline 并返回 Timeout（供 Engine 重标）。
    /// 被截断时**剩余时长会被保留**，下次拉取接着睡——截断不等于消费掉整段延迟。
    Delay(Duration),
    /// 传输 / 协议层失败。
    Fail(ModelError),
    /// 显式正常 EOF。
    Eof,
}

/// 可克隆脚本；每次 `open` 产生一份独立游标。
#[derive(Debug, Clone, Default)]
pub struct Script {
    steps: Vec<Step>,
}

impl Script {
    pub fn builder() -> ScriptBuilder {
        ScriptBuilder(Vec::new())
    }

    /// 生成一份独立游标的事件流。
    pub fn into_stream(self) -> ScriptedStream {
        ScriptedStream {
            steps: self.steps.into_iter(),
            pending_delay: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScriptBuilder(Vec<Step>);

impl ScriptBuilder {
    pub fn emit(mut self, event: ModelEvent) -> Self {
        self.0.push(Step::Emit(event));
        self
    }

    pub fn delay(mut self, d: Duration) -> Self {
        self.0.push(Step::Delay(d));
        self
    }

    pub fn fail(mut self, e: ModelError) -> Self {
        self.0.push(Step::Fail(e));
        self
    }

    pub fn eof(mut self) -> Self {
        self.0.push(Step::Eof);
        self
    }

    pub fn build(self) -> Script {
        Script { steps: self.0 }
    }
}

pub struct ScriptedStream {
    steps: std::vec::IntoIter<Step>,
    /// 被 deadline 截断后**尚未睡满**的剩余延迟。
    ///
    /// 必须保留：截断不能吞掉延迟，否则下一次拉取会直接跳过整段等待。
    /// 曾因此让 `cancellation_mid_stream_interrupts_wait` 在负载高的 CI runner
    /// 上偶发失败——引擎先拿到"跳过延迟后的 completed"，终结事件就成了 Completed。
    pending_delay: Option<Duration>,
}

impl ProviderStream for ScriptedStream {
    fn next_event(&mut self, deadline: Instant) -> Result<Option<ModelEvent>, ModelError> {
        loop {
            // 上次被截断的剩余延迟优先；它不再属于 steps，不会被重复消费。
            let delay = match self.pending_delay.take() {
                Some(rest) => rest,
                None => match self.steps.next() {
                    None => return Ok(None), // 未显式 eof 也视为正常 EOF
                    Some(Step::Emit(e)) => return Ok(Some(e)),
                    Some(Step::Eof) => return Ok(None),
                    Some(Step::Fail(e)) => return Err(e),
                    Some(Step::Delay(d)) => d,
                },
            };
            if let Some(rest) = sleep_respecting_deadline(delay, deadline) {
                self.pending_delay = Some(rest);
                return Err(ModelError::Timeout {
                    kind: TimeoutKind::Idle,
                    detail: ErrorDetail::new("scripted delay exceeded deadline"),
                });
            }
        }
    }
}

/// 睡满 `d` 返回 `None`；deadline 先到则睡到 deadline 并返回**剩余**时长。
fn sleep_respecting_deadline(d: Duration, deadline: Instant) -> Option<Duration> {
    let now = Instant::now();
    let until = now + d;
    if until <= deadline {
        thread::sleep(d);
        None
    } else {
        let slept = deadline.saturating_duration_since(now);
        if !slept.is_zero() {
            thread::sleep(slept);
        }
        Some(d.saturating_sub(slept))
    }
}

/// 每次打开都产生新游标的工厂；`open_failures` 中的错误依次消耗，
/// 模拟 Retry 场景的前 N 次 open 失败。
#[derive(Debug, Default)]
pub struct ScriptedFactory {
    script: Script,
    open_failures: std::sync::Mutex<VecDeque<ModelError>>,
}

impl ScriptedFactory {
    pub fn new(script: Script) -> Self {
        Self {
            script,
            open_failures: std::sync::Mutex::new(VecDeque::new()),
        }
    }

    /// 前缀失败：前 `n` 次 open 返回 `err`，之后正常。
    pub fn with_open_failures(self, n: usize, err: ModelError) -> Self {
        {
            let mut failures = self.open_failures.lock().expect("factory mutex");
            for _ in 0..n {
                failures.push_back(err.clone());
            }
        }
        self
    }
}

impl SourceFactory for ScriptedFactory {
    fn open(&self) -> Result<Box<dyn ProviderStream>, ModelError> {
        let mut failures = self.open_failures.lock().expect("factory mutex");
        if let Some(e) = failures.pop_front() {
            return Err(e);
        }
        drop(failures);
        Ok(Box::new(ScriptedStream {
            steps: self.script.steps.clone().into_iter(),
            pending_delay: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umber_core::ids::BlockId;

    #[test]
    fn eof_when_script_exhausted() {
        let mut s = Script::builder()
            .emit(ModelEvent::TextStarted {
                block_id: BlockId::from("b"),
            })
            .build()
            .into_stream();
        let deadline = Instant::now() + Duration::from_secs(1);
        assert!(s.next_event(deadline).unwrap().is_some());
        assert!(s.next_event(deadline).unwrap().is_none());
    }

    /// 回归：被 deadline 截断不得吞掉延迟。
    ///
    /// 否则下一次拉取会跳过整段等待直接吐出后续事件——这正是
    /// `cancellation_mid_stream_interrupts_wait` 在负载高的 runner 上偶发失败的原因。
    #[test]
    fn truncated_delay_is_not_swallowed() {
        let mut s = Script::builder()
            .delay(Duration::from_millis(300))
            .emit(ModelEvent::TextStarted {
                block_id: BlockId::from("b"),
            })
            .build()
            .into_stream();

        let short = || Instant::now() + Duration::from_millis(20);
        assert!(s.next_event(short()).is_err(), "第一次应被截断并报超时");
        assert!(
            s.next_event(short()).is_err(),
            "剩余延迟必须继续生效，而不是被跳过"
        );

        // 睡满剩余延迟之后才允许吐出事件
        let long = Instant::now() + Duration::from_secs(2);
        assert!(s.next_event(long).unwrap().is_some());
    }

    #[test]
    fn delay_respects_deadline() {
        let mut s = Script::builder()
            .delay(Duration::from_secs(5))
            .build()
            .into_stream();
        let deadline = Instant::now() + Duration::from_millis(30);
        let err = s.next_event(deadline).unwrap_err();
        assert_eq!(err.kind_name(), "timeout");
    }
}
