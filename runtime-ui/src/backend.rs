//! 设置界面与 Core 之间的动作接口（总案 §38：配置与状态逻辑全在 Core，
//! UI 只渲染 UISpec 并调用这里定义的动作）。
//!
//! 参考实现（runtime-ui-egui）只依赖本模块的数据类型——
//! 因此换渲染层时，宿主的业务接线原样保留。

use runtime_model::capability::{CapabilityKind, CapabilityStatus};
use serde::{Deserialize, Serialize};

use crate::discovery::UiModelEntry;
use crate::schema::SettingsDraft;

/// 连接测试结果（对应 §59 的"测试连接"）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionReport {
    pub latency_ms: u64,
}

/// 连接测试的 UI 状态机（与 DiscoverySession 同构的契约状态）。
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
/// `detail` 是原文诊断（已由调用方负责脱敏，§28）。
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

/// 供 UI 展示的模型知识（能力显示，总案 §59）。
///
/// 未记录的能力照实呈现 Unknown——UI 层绝不美化（§15 §16）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiModelInfo {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub capabilities: Vec<(CapabilityKind, CapabilityStatus)>,
}

impl UiModelInfo {
    /// 全 Unknown 的诚实条目（Passive 探测无法确认能力时的正确形态）。
    pub fn all_unknown(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            display_name: None,
            capabilities: vec![],
        }
    }
}

/// 设置窗口的宿主接线。全部方法阻塞式——UI 层负责放到工作线程，
/// 以保证渲染线程不因网络而卡顿（egui 立即模式的要求）。
pub trait SettingsBackend: Send + Sync {
    /// 测试连接。语义必须是 Passive（§17.1）：不得发送生成请求。
    fn test_connection(&self, draft: &SettingsDraft) -> Result<ConnectionReport, BackendError>;

    /// 模型发现（GET /models；失败可手动回退，§36）。
    fn discover(&self, draft: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError>;

    /// 已知模型知识（可返回 None：UI 显示 Unknown）。
    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        let _ = model_id;
        None
    }
}
