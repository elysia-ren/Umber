//! 统一消息模型（总案 §19）：`role` 由 Runtime 自身定义，内容一律用统一内容块。

use serde::{Deserialize, Serialize};

use crate::content::{ContentBlock, TextBlock};

/// 对话角色。命名是 Canonical 语义，与任何 Provider 的字面命名无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 一条消息 = 角色 + 统一内容块序列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn new(role: Role, content: Vec<ContentBlock>) -> Self {
        Self { role, content }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, vec![ContentBlock::Text(TextBlock::new(text))])
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentBlock::Text(TextBlock::new(text))])
    }

    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Self::new(Role::Assistant, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_is_snake_case_on_wire() {
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
    }
}
