//! Canonical Request（总案 §18）。
//!
//! 原则：统一语义，不做最低公约数——可以表达主流模型真正具有的共同能力，
//! 而不是为了最弱 Provider 把 API 降级成纯文本生成。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ids::DeploymentId;
use crate::message::Message;

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Canonical 生成请求（总案 §18）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// 目标 Deployment（总案 §8：不是裸模型 ID）。
    pub model: DeploymentId,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tool_choice: ToolChoice,
    #[serde(default, skip_serializing_if = "is_default")]
    pub reasoning: ReasoningConfig,
    #[serde(default, skip_serializing_if = "is_default")]
    pub generation: GenerationConfig,
    #[serde(default, skip_serializing_if = "is_default")]
    pub modalities: OutputModalities,
    #[serde(default, skip_serializing_if = "is_default")]
    pub response_format: ResponseFormat,
    #[serde(default, skip_serializing_if = "is_default")]
    pub caching: CachingConfig,
    /// 宿主追踪与透传。凭据绝不进入此字段（总案 §32）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

/// Canonical 工具定义（总案 §21）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema 对象。
    pub input_schema: serde_json::Value,
}

/// 工具选择策略（总案 §22.1）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
    /// 不支持的原生 Provider 由 Adapter 模拟并标记 emulated；无法保证则映射为 Unsupported。
    Required,
    Specific {
        name: String,
    },
}

/// 推理配置（总案 §21.1）：统一档位，Adapter 负责到各家参数的映射，
/// Provider 不支持的档位就近降级并写入 CompatibilityProfile。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub effort: ReasoningEffort,
    /// true 表示不返回推理内容。
    #[serde(default)]
    pub exclude: bool,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            effort: ReasoningEffort::Medium,
            exclude: false,
        }
    }
}

/// 统一推理档位。
///
/// **顺序即强弱**（Minimal < Low < Medium < High）——就近降级依赖该顺序
/// （总案 §21.1，实现见 umber-model::effort）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// 全部档位（按强弱）。
    pub fn all() -> [Self; 4] {
        [Self::Minimal, Self::Low, Self::Medium, Self::High]
    }

    /// 协议侧使用的档位名（openai_chat / openai_responses 直接发送该值）。
    pub fn canonical_label(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// 采样与生成长度参数。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerationConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// 输出模态声明。V1 仅承诺文本输出（总案 §61：图像 / 音频生成暂缓）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputModalities {
    pub text: bool,
}

impl Default for OutputModalities {
    fn default() -> Self {
        Self { text: true }
    }
}

/// 结构化输出（总案 §18.1）：Canonical 使用 JSON Schema 方言，
/// 各协议映射差异由 Adapter 消化并写入 CompatibilityProfile。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseFormat {
    #[default]
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        /// 对应 OpenAI strict 语义；不支持的 Provider 按 partial 处理。
        #[serde(default)]
        strict: bool,
    },
}

/// 缓存策略（总案 §20）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachingConfig {
    pub mode: CacheMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    /// 交由 Provider 自动缓存，Runtime 不发多余字段。
    #[default]
    Auto,
    /// 尊重内容块上的 `cache_control` 标注（Anthropic 断点式）。
    Manual,
}

/// 请求级语义校验。协议差异类问题（某 Provider 不支持某参数）不属于此检查，
/// 那是 Adapter / CompatibilityProfile 的职责。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestValidationError {
    pub reason: String,
}

impl std::fmt::Display for RequestValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid request: {}", self.reason)
    }
}

impl std::error::Error for RequestValidationError {}

impl GenerateRequest {
    pub fn new(model: impl Into<DeploymentId>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: Vec::new(),
            tool_choice: ToolChoice::default(),
            reasoning: ReasoningConfig::default(),
            generation: GenerationConfig::default(),
            modalities: OutputModalities::default(),
            response_format: ResponseFormat::default(),
            caching: CachingConfig::default(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), RequestValidationError> {
        let err = |reason: &str| RequestValidationError {
            reason: reason.to_string(),
        };

        if !self.modalities.text {
            return Err(err("V1 仅支持文本输出模态（总案 §61）"));
        }
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            if !names.insert(tool.name.as_str()) {
                return Err(err(&format!("duplicate tool name: {}", tool.name)));
            }
        }
        if let ResponseFormat::JsonSchema { schema, .. } = &self.response_format {
            if !schema.is_object() {
                return Err(err("json_schema response format requires a schema object"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_serialize_compactly() {
        let req = GenerateRequest::new("deploy-1", vec![Message::user("你好")]);
        let json = serde_json::to_string(&req).unwrap();
        // default 字段不落盘，保持线上格式干净
        assert!(!json.contains("tool_choice"));
        assert!(!json.contains("reasoning"));
        let back: GenerateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn rejects_duplicate_tool_names() {
        let mut req = GenerateRequest::new("d", vec![Message::user("hi")]);
        let tool = |name: &str| Tool {
            name: name.into(),
            description: String::new(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        req.tools = vec![tool("a"), tool("a")];
        assert!(req.validate().is_err());
    }

    #[test]
    fn rejects_non_object_schema() {
        let mut req = GenerateRequest::new("d", vec![Message::user("hi")]);
        req.response_format = ResponseFormat::JsonSchema {
            name: "out".into(),
            schema: serde_json::json!([]),
            strict: true,
        };
        assert!(req.validate().is_err());
    }
}
