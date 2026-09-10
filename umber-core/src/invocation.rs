//! Invocation 生命周期（总案 §25）与部分结果保证（§25.1）。

use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;
use crate::error::ModelError;
use crate::ids::InvocationId;
use crate::request::GenerateRequest;
use crate::usage::Usage;

/// Invocation 状态（总案 §25）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationState {
    Created,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl InvocationState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            InvocationState::Completed | InvocationState::Failed | InvocationState::Cancelled
        )
    }
}

/// 非法状态迁移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: InvocationState,
    pub to: InvocationState,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid invocation transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidTransition {}

/// 一次模型调用的生命周期记录（总案 §25）。
///
/// 时间统一为 Unix 毫秒，避免引入时间库依赖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub id: InvocationId,
    pub request: GenerateRequest,
    pub state: InvocationState,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
}

impl Invocation {
    pub fn new(id: impl Into<InvocationId>, request: GenerateRequest, now_ms: u64) -> Self {
        Self {
            id: id.into(),
            request,
            state: InvocationState::Created,
            created_at_ms: now_ms,
            started_at_ms: None,
            ended_at_ms: None,
        }
    }

    /// 合法迁移：Created → Running → {Completed, Failed, Cancelled}。
    /// 终态不可再迁移。
    pub fn transition(&mut self, to: InvocationState) -> Result<(), InvalidTransition> {
        let allowed: &[InvocationState] = match self.state {
            InvocationState::Created => &[InvocationState::Running],
            InvocationState::Running => &[
                InvocationState::Completed,
                InvocationState::Failed,
                InvocationState::Cancelled,
            ],
            InvocationState::Completed | InvocationState::Failed | InvocationState::Cancelled => {
                &[]
            }
        };
        if !allowed.contains(&to) {
            return Err(InvalidTransition {
                from: self.state,
                to,
            });
        }
        if to == InvocationState::Running {
            self.started_at_ms = Some(self.started_at_ms.unwrap_or(self.created_at_ms));
        }
        if to.is_terminal() {
            self.ended_at_ms = Some(
                self.ended_at_ms
                    .unwrap_or_else(|| self.started_at_ms.unwrap_or(self.created_at_ms)),
            );
        }
        self.state = to;
        Ok(())
    }
}

/// 部分结果（总案 §25.1）：Invocation 进入 Failed 或 Cancelled 后，
/// 宿主必须仍能取回已聚合的部分输出与已知 Usage。Runtime 不得丢弃。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PartialOutput {
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_error: Option<ModelError>,
}

impl PartialOutput {
    pub fn is_empty(&self) -> bool {
        self.content.is_empty() && self.usage.is_none()
    }

    /// 聚合后的文本内容（诊断用）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for block in &self.content {
            if let Some(text) = block.as_text() {
                out.push_str(text);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;

    fn invocation() -> Invocation {
        Invocation::new(
            "inv-1",
            GenerateRequest::new("d", vec![Message::user("hi")]),
            1_000,
        )
    }

    #[test]
    fn lifecycle_transitions_are_enforced() {
        let mut inv = invocation();
        assert!(inv.transition(InvocationState::Completed).is_err());
        assert!(inv.transition(InvocationState::Running).is_ok());
        assert!(inv.transition(InvocationState::Running).is_err());
        assert!(inv.transition(InvocationState::Cancelled).is_ok());
        assert!(inv.transition(InvocationState::Running).is_err());
        assert_eq!(inv.state, InvocationState::Cancelled);
        assert!(inv.ended_at_ms.is_some());
    }

    #[test]
    fn terminal_states_match_terminal_events() {
        assert!(InvocationState::Failed.is_terminal());
        assert!(!InvocationState::Running.is_terminal());
    }
}
