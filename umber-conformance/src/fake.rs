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
}

impl ProviderStream for ScriptedStream {
    fn next_event(&mut self, deadline: Instant) -> Result<Option<ModelEvent>, ModelError> {
        loop {
            match self.steps.next() {
                None => return Ok(None), // 未显式 eof 也视为正常 EOF
                Some(Step::Emit(e)) => return Ok(Some(e)),
                Some(Step::Eof) => return Ok(None),
                Some(Step::Fail(e)) => return Err(e),
                Some(Step::Delay(d)) => {
                    if !sleep_respecting_deadline(d, deadline) {
                        return Err(ModelError::Timeout {
                            kind: TimeoutKind::Idle,
                            detail: ErrorDetail::new("scripted delay exceeded deadline"),
                        });
                    }
                }
            }
        }
    }
}

/// 睡满 `d` 返回 true；deadline 先到则睡到 deadline 返回 false。
fn sleep_respecting_deadline(d: Duration, deadline: Instant) -> bool {
    let until = Instant::now() + d;
    if until <= deadline {
        thread::sleep(d);
        true
    } else {
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        }
        false
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
