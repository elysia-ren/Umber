//! Canonical Response（总案 §27）。
//!
//! 宿主拿到的最终响应不是裸文本：`stop_reason` 是宿主判断 Agent Loop
//! 是否继续的唯一依据，任何协议必须映射到该枚举。

use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;
use crate::ids::InvocationId;
use crate::usage::Usage;

/// 停止原因（总案 §27）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// 自然结束。
    EndTurn,
    /// 达到输出上限。
    MaxTokens,
    /// 等待宿主执行工具。
    ToolUse,
    /// 模型拒绝。
    Refusal,
    /// 安全拦截。
    SafetyBlocked,
}

/// 诊断上下文。`raw_context` 输出前必须已脱敏（总案 §28）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_context: Option<serde_json::Value>,
}

/// 最终响应（总案 §27）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateResponse {
    pub invocation_id: InvocationId,
    /// 完整内容块（Text / Reasoning / ToolCall ...）。
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    pub usage: Usage,
    #[serde(default)]
    pub provider_context: ProviderContext,
}

impl GenerateResponse {
    /// 拼接全部文本块内容。
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for block in &self.content {
            if let Some(text) = block.as_text() {
                out.push_str(text);
            }
        }
        out
    }

    /// 全部工具调用块。
    pub fn tool_calls(&self) -> impl Iterator<Item = &crate::content::ToolCallBlock> {
        self.content.iter().filter_map(|b| match b {
            ContentBlock::ToolCall(c) => Some(c),
            _ => None,
        })
    }
}
