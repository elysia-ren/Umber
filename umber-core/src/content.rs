//! 统一内容块（总案 §19）。
//!
//! 禁止出现 `DeepSeekReasoningContent` / `AnthropicThinkingBlock` /
//! `OpenAIResponseItem` 这类 Provider 类型；Provider 差异由 Adapter 消化。

use serde::{Deserialize, Serialize};

use crate::ids::CallId;

/// 厂商推理透传载荷（总案 §19.1）。
///
/// Runtime 原样保存、原样回传，不解释、不修改。
/// 宿主丢弃它是合法的，但部分 Provider 可能因此拒绝多轮工具调用，
/// 后果写入 CompatibilityProfile。
pub type ProviderPayload = serde_json::Value;

/// 内容块级缓存标注（总案 §20）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheControl {
    /// Anthropic 风格的显式缓存断点；OpenAI 为自动缓存，Adapter 不发多余字段。
    Ephemeral,
}

/// 媒体来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MediaSource {
    /// 远程 URL，由 Provider 拉取。
    Url { url: String },
    /// 内联 Base64 数据。
    Base64 { media_type: String, data: String },
}

/// 统一内容块。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Audio(AudioBlock),
    Video(VideoBlock),
    ToolCall(ToolCallBlock),
    ToolResult(ToolResultBlock),
    Reasoning(ReasoningBlock),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl TextBlock {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache_control: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageBlock {
    pub source: MediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioBlock {
    pub source: MediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoBlock {
    pub source: MediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// 模型产生的工具调用。`arguments_json` 是原始参数 JSON 文本，供增量重组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallBlock {
    pub call_id: CallId,
    pub name: String,
    pub arguments_json: String,
}

/// 宿主执行工具后提交的结果（总案 §21：Runtime 不执行 Tool，只统一传输语义）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultBlock {
    pub call_id: CallId,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

/// 推理内容块（总案 §20）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningBlock {
    pub text: String,
    /// 可选 opaque 载荷，原样透传（§19.1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_payload: Option<ProviderPayload>,
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextBlock::new(text))
    }

    /// 若为文本块，返回其文本。
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text(t) => Some(&t.text),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_payload_roundtrips_verbatim() {
        let block = ContentBlock::Reasoning(ReasoningBlock {
            text: "思考过程".into(),
            provider_payload: Some(serde_json::json!({ "signature": "abc", "blob": [1, 2] })),
        });
        let json = serde_json::to_string(&block).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back, block);
        // 透传字段结构原样保留
        if let ContentBlock::Reasoning(r) = &back {
            assert_eq!(
                r.provider_payload.as_ref().unwrap()["signature"],
                "abc".to_string()
            );
        } else {
            panic!("expected reasoning block");
        }
    }

    #[test]
    fn tool_result_supports_nested_blocks() {
        let result = ContentBlock::ToolResult(ToolResultBlock {
            call_id: CallId::from("call-1"),
            is_error: true,
            content: vec![ContentBlock::text("boom")],
        });
        let back: ContentBlock =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert_eq!(back, result);
    }
}
