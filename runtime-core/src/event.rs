//! Canonical Event（总案 §23–§24）。
//!
//! - 所有 Delta 均为 append-only 增量，不是 replacement。
//! - 同一 Invocation 内所有事件共享一个全局单调递增 `sequence`。
//! - 终结事件保证（§23.1）：无论底层发生什么——正常结束、超时、取消、
//!   断流、畸形 SSE——每个 Invocation 恰好产生一个终结事件
//!   （Completed / Failed / Cancelled）。

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::{BlockId, CallId, InvocationId};
use crate::response::GenerateResponse;
use crate::usage::Usage;

/// 带全局序号的事件。`sequence` 在 Invocation 内严格单调递增（总案 §24）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub sequence: u64,
    pub event: ModelEvent,
}

/// 模型事件流。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelEvent {
    Started {
        invocation_id: InvocationId,
    },
    ReasoningStarted {
        block_id: BlockId,
    },
    ReasoningDelta {
        block_id: BlockId,
        delta: String,
    },
    ReasoningEnded {
        block_id: BlockId,
    },
    TextStarted {
        block_id: BlockId,
    },
    TextDelta {
        block_id: BlockId,
        delta: String,
    },
    TextEnded {
        block_id: BlockId,
    },
    ToolCallStarted {
        call_id: CallId,
        name: String,
    },
    ToolCallDelta {
        call_id: CallId,
        arguments_json_delta: String,
    },
    ToolCallFinished {
        call_id: CallId,
    },
    UsageUpdated {
        usage: Usage,
    },
    Completed {
        response: Box<GenerateResponse>,
    },
    Failed {
        error: ModelError,
    },
    Cancelled,
}

impl ModelEvent {
    /// 是否为终结事件。每个 Invocation 恰好一个（总案 §23.1）。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ModelEvent::Completed { .. } | ModelEvent::Failed { .. } | ModelEvent::Cancelled
        )
    }
}

/// 全局 sequence 单调性校验器。Conformance 与宿主侧重组共用。
#[derive(Debug, Clone, Default)]
pub struct SequenceValidator {
    last: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceError {
    pub previous: Option<u64>,
    pub received: u64,
}

impl std::fmt::Display for SequenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sequence not monotonic: previous={:?} received={}",
            self.previous, self.received
        )
    }
}

impl std::error::Error for SequenceError {}

impl SequenceValidator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 校验严格递增；通过则记录该序号。
    pub fn check(&mut self, sequence: u64) -> Result<(), SequenceError> {
        if let Some(last) = self.last {
            if sequence <= last {
                return Err(SequenceError {
                    previous: Some(last),
                    received: sequence,
                });
            }
        }
        self.last = Some(sequence);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evt(delta: &str) -> ModelEvent {
        ModelEvent::TextDelta {
            block_id: BlockId::from("b1"),
            delta: delta.to_string(),
        }
    }

    #[test]
    fn terminal_events_are_exactly_three_kinds() {
        assert!(!evt("x").is_terminal());
        assert!(ModelEvent::Cancelled.is_terminal());
        assert!(ModelEvent::Failed {
            error: ModelError::EmptyResponse
        }
        .is_terminal());
    }

    #[test]
    fn sequence_must_be_strictly_increasing() {
        let mut v = SequenceValidator::new();
        assert!(v.check(0).is_ok());
        assert!(v.check(1).is_ok());
        assert_eq!(
            v.check(1),
            Err(SequenceError {
                previous: Some(1),
                received: 1
            })
        );
    }

    #[test]
    fn event_roundtrips_through_json() {
        let e = SequencedEvent {
            sequence: 7,
            event: evt("你好"),
        };
        let back: SequencedEvent =
            serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }
}
