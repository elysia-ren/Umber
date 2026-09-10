//! 模型发现流程状态机（总案 §36：Discovery 失败不是硬性阻断）。

use serde::{Deserialize, Serialize};

/// 供 UI 渲染的模型条目（UISpec 自有形状，不泄漏 Provider 层类型）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiModelEntry {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Discovery 会话状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DiscoveryState {
    Idle,
    Discovering,
    /// 发现失败：允许手动添加 Model ID（总案 §36——没有 /models 不阻断使用）
    Failed {
        reason_key: String,
    },
    Done {
        models: Vec<UiModelEntry>,
    },
}

/// Discovery 会话：Core 持有状态与转移，宿主只读状态渲染 UI。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoverySession {
    state: DiscoveryState,
}

impl DiscoverySession {
    pub fn new() -> Self {
        Self {
            state: DiscoveryState::Idle,
        }
    }

    pub fn state(&self) -> &DiscoveryState {
        &self.state
    }

    pub fn begin(&mut self) -> Result<(), &'static str> {
        match self.state {
            DiscoveryState::Idle | DiscoveryState::Failed { .. } | DiscoveryState::Done { .. } => {
                self.state = DiscoveryState::Discovering;
                Ok(())
            }
            DiscoveryState::Discovering => Err("discovery already in progress"),
        }
    }

    pub fn succeed(&mut self, models: Vec<UiModelEntry>) -> Result<(), &'static str> {
        if !matches!(self.state, DiscoveryState::Discovering) {
            return Err("not discovering");
        }
        self.state = DiscoveryState::Done { models };
        Ok(())
    }

    pub fn fail(&mut self, reason_key: &str) -> Result<(), &'static str> {
        if !matches!(self.state, DiscoveryState::Discovering) {
            return Err("not discovering");
        }
        self.state = DiscoveryState::Failed {
            reason_key: reason_key.to_string(),
        };
        Ok(())
    }

    /// 发现失败 → 手动添加 Model ID（总案 §36）。
    pub fn add_manual_model(&mut self, model_id: &str) -> Result<(), &'static str> {
        if model_id.trim().is_empty() {
            return Err("model id is empty");
        }
        match &mut self.state {
            DiscoveryState::Failed { .. } | DiscoveryState::Idle => {
                self.state = DiscoveryState::Done {
                    models: vec![UiModelEntry {
                        model_id: model_id.trim().to_string(),
                        display_name: None,
                    }],
                };
                Ok(())
            }
            DiscoveryState::Done { models } => {
                models.push(UiModelEntry {
                    model_id: model_id.trim().to_string(),
                    display_name: None,
                });
                Ok(())
            }
            DiscoveryState::Discovering => Err("discovery in progress"),
        }
    }
}

impl Default for DiscoverySession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_lifecycle_with_manual_fallback() {
        let mut session = DiscoverySession::new();
        session.begin().unwrap();
        // 发现失败不是阻断：手动添加 Model ID（总案 §36）
        session.fail("discovery.no_model_list").unwrap();
        session.add_manual_model("deepseek-chat").unwrap();
        match session.state() {
            DiscoveryState::Done { models } => assert_eq!(models[0].model_id, "deepseek-chat"),
            other => panic!("unexpected state {other:?}"),
        }
        // 可以再次发起
        session.begin().unwrap();
        assert!(matches!(session.state(), DiscoveryState::Discovering));
    }

    #[test]
    fn cannot_succeed_when_not_discovering() {
        let mut session = DiscoverySession::new();
        assert!(session.succeed(vec![]).is_err());
    }
}
