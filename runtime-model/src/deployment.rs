//! Provider → Endpoint → Deployment 链（总案 §6–§8 §35）。
//!
//! 铁律：Provider ≠ Protocol。任何 Provider × Protocol × Endpoint 组合合法；
//! Preset 数据（而非本类型）负责反映厂商实际公开端点。

use serde::{Deserialize, Serialize};

use runtime_core::DeploymentId;

/// 原生外部协议（总案 §4）。
///
/// `ProviderNative` 是厂商私有协议扩展槽，其实现最终仍必须转换成
/// Canonical API。"OpenAI Compatible" 不是协议类型，一律映射为
/// `OpenAiChat` + CompatibilityProfile（总案 §5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolKind {
    OpenAiResponses,
    OpenAiChat,
    AnthropicMessages,
    Gemini,
    ProviderNative,
}

/// 连接地址。Endpoint 必须允许覆盖（总案 §34），Preset 绝不锁死 Endpoint。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub provider_id: String,
    pub url: String,
}

/// 一个 Endpoint 上具体暴露出的模型服务（总案 §8）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub id: DeploymentId,
    pub endpoint_id: String,
    pub protocol: ProtocolKind,
    /// Provider 侧的模型 ID。
    pub model_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_kind_is_snake_case_on_wire() {
        assert_eq!(
            serde_json::to_string(&ProtocolKind::AnthropicMessages).unwrap(),
            "\"anthropic_messages\""
        );
        let back: ProtocolKind = serde_json::from_str("\"provider_native\"").unwrap();
        assert_eq!(back, ProtocolKind::ProviderNative);
    }
}
