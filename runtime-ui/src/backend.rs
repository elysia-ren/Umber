//! 设置界面与 Core 之间的动作接口（总案 §38：配置与状态逻辑全在 Core，
//! UI 只渲染数据并调用这里定义的动作）。
//!
//! 参考实现（runtime-ui-egui）只依赖本模块的数据类型——
//! 因此换渲染层时，宿主的业务接线原样保留。

use runtime_core::request::ReasoningEffort;
use runtime_model::capability::{CapabilityKind, CapabilityStatus};
use runtime_model::model::{ModelLimits, Pricing};
use runtime_model::EvidenceSummary;
use serde::{Deserialize, Serialize};

use crate::discovery::UiModelEntry;
use crate::schema::SettingsDraft;

/// 连接测试结果（对应 §59 的"测试连接"）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionReport {
    pub latency_ms: u64,
}

/// 连接测试的 UI 状态机。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionTestState {
    #[default]
    Idle,
    Testing,
    Ok {
        latency_ms: u64,
    },
    Failed {
        reason_key: String,
    },
}

/// 后端动作失败。`reason_key` 必须能在 Strings 里渲染；
/// `detail` 是原文诊断（调用方负责脱敏，§28）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    pub reason_key: String,
    pub detail: String,
}

impl BackendError {
    pub fn new(reason_key: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            reason_key: reason_key.into(),
            detail: detail.into(),
        }
    }
}

/// 供界面展示的模型知识（Model Intelligence 的呈现层）。
///
/// 未记录的能力照实呈现 Unknown——界面绝不美化（§15 §16）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiModelInfo {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 只包含**已知**的能力；空 = 目录里没有该模型或目录无此字段。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<(CapabilityKind, CapabilityStatus)>,
    #[serde(default)]
    pub limits: ModelLimits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<Pricing>,
    /// 该模型实际支持的思考强度档位（空 = 未知，不做本地降级）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_efforts: Vec<ReasoningEffort>,
    /// 证据摘要：这个值是谁给的（§X.25）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceSummary>,
}

impl UiModelInfo {
    /// 全未知的诚实条目（目录里没有这个模型时的正确形态）。
    pub fn unknown(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            display_name: None,
            capabilities: Vec::new(),
            limits: ModelLimits::default(),
            pricing: None,
            supported_efforts: Vec::new(),
            evidence: Vec::new(),
        }
    }

    /// 目录里是否有这个模型的任何知识。
    pub fn has_data(&self) -> bool {
        !self.capabilities.is_empty()
            || self.limits.context_window.is_some()
            || self.pricing.is_some()
    }
}

/// 设置窗口的宿主接线。全部方法阻塞式——UI 层负责放到工作线程，
/// 以保证渲染线程不因网络而卡顿（egui 立即模式的要求）。
pub trait SettingsBackend: Send + Sync {
    /// 测试连接。语义必须是 Passive（§17.1）：不得发送生成请求。
    fn test_connection(&self, draft: &SettingsDraft) -> Result<ConnectionReport, BackendError>;

    /// 模型发现（GET /models；失败可手动回退，§36）。
    fn discover(&self, draft: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError>;

    /// 已知模型知识（返回 None = 目录里没有；界面显示"无数据"而不是空白）。
    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        let _ = model_id;
        None
    }

    /// 请求 URL 预览（"请求将发送到 …"）。
    ///
    /// 由后端计算而不是 UI 拼接——**保证预览与 Adapter 实际使用的 URL
    /// 出自同一处逻辑**，否则预览会随代码演进变成谎言。
    fn preview_request_url(&self, draft: &SettingsDraft) -> Option<String> {
        let _ = draft;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_model_has_no_data_and_no_invented_values() {
        let info = UiModelInfo::unknown("mystery-model");
        assert!(!info.has_data());
        assert!(info.capabilities.is_empty());
        assert_eq!(info.limits.context_window, None);
        assert!(info.pricing.is_none());
        assert!(info.supported_efforts.is_empty());
    }

    #[test]
    fn only_two_methods_are_mandatory() {
        // 宿主接入成本要低：默认实现让可选能力零成本
        struct Minimal;
        impl SettingsBackend for Minimal {
            fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
                Ok(ConnectionReport { latency_ms: 1 })
            }
            fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
                Ok(vec![])
            }
        }
        let backend = Minimal;
        assert!(backend.model_info("x").is_none());
        assert!(backend
            .preview_request_url(&SettingsDraft::default())
            .is_none());
    }
}
