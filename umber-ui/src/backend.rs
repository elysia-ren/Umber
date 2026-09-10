//! 设置界面与 Core 之间的动作接口（总案 §38：配置与状态逻辑全在 Core，
//! UI 只渲染数据并调用这里定义的动作）。
//!
//! 参考实现（umber-ui-egui）只依赖本模块的数据类型——
//! 因此换渲染层时，宿主的业务接线原样保留。

use serde::{Deserialize, Serialize};
use umber_core::request::ReasoningEffort;
use umber_model::capability::{CapabilityKind, CapabilityStatus};
use umber_model::model::{ModelLimits, Pricing};
use umber_model::EvidenceSummary;

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

/// 已保存的配置（**不含密钥**）。
///
/// 密钥只以引用形式存在于凭据存储，回读时只能知道"有没有"，
/// 永远不把密钥值交回界面层（§32）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSettings {
    pub provider: String,
    pub protocol: String,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// 凭据存储里是否已有这个 provider 的密钥。
    #[serde(default)]
    pub has_api_key: bool,
}

/// 保存结果（供界面如实提示"保存了什么"）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveReport {
    /// 配置（provider/protocol/endpoint/model/上下文覆盖）是否已落盘。
    pub saved_config: bool,
    /// 密钥是否已写入凭据存储。
    pub saved_credential: bool,
    /// 凭据存储层级告警 i18n key：落到加密文件层时必须提示用户（§32.1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_warning_key: Option<String>,
}

/// 供界面展示的模型知识（Model Intelligence 的呈现层）。
///
/// 未记录的能力照实呈现 Unknown——界面绝不美化（§15 §16）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiModelInfo {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 归属组织 / 上游 provider 键（用于"按厂商查目录推荐"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
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
            provider: None,
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
    ///
    /// `api_key` 是**本次会话**用户刚输入的密钥（`None` = 沿用已保存的）。
    /// 它与 `draft` 分开传递，因为：
    /// - `draft` 会被序列化（配置落盘），绝不能含密钥（§32）
    /// - 密钥只在内存里活一次请求，用完即弃，不进任何持久化结构
    fn test_connection(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<ConnectionReport, BackendError>;

    /// 模型发现（GET /models；失败可手动回退，§36）。`api_key` 同上。
    fn discover(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<Vec<UiModelEntry>, BackendError>;

    /// 已知模型知识（返回 None = 目录里没有；界面显示"无数据"而不是空白）。
    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        let _ = model_id;
        None
    }

    /// **按厂商**从随包目录取推荐模型（替代预置里硬编码模型名）。
    ///
    /// `catalog_provider_ids` 来自 `ProviderPreset`，是上游目录里的 provider 键
    /// （如 `zhipuai` / `moonshotai`）。目录里没有该厂商时返回空——界面据此
    /// 提示"刷新模型列表"或让用户手填，而不是显示过时的模型名。
    fn recommend_models(&self, catalog_provider_ids: &[String], limit: usize) -> Vec<UiModelEntry> {
        let _ = (catalog_provider_ids, limit);
        Vec::new()
    }

    /// 请求 URL 预览（"请求将发送到 …"）。
    ///
    /// 由后端计算而不是 UI 拼接——**保证预览与 Adapter 实际使用的 URL
    /// 出自同一处逻辑**，否则预览会随代码演进变成谎言。
    fn preview_request_url(&self, draft: &SettingsDraft) -> Option<String> {
        let _ = draft;
        None
    }

    /// 读取已保存的配置（启动时恢复界面）。
    ///
    /// 返回 `None` 表示从未保存过——首启是正常状态，不是错误。
    fn load_settings(&self) -> Option<SavedSettings> {
        None
    }

    /// 保存配置。
    ///
    /// 职责划分（§31 §32）：
    /// - 配置项（provider/protocol/endpoint/model/上下文覆盖）→ Runtime Local DB
    /// - `api_key` → **凭据存储**，绝不写进配置
    ///
    /// `api_key` 为空表示"不改动已保存的密钥"（用户没重新输入时）。
    fn save_settings(
        &self,
        draft: &SettingsDraft,
        api_key: &str,
    ) -> Result<SaveReport, BackendError> {
        let _ = (draft, api_key);
        Err(BackendError::new(
            "settings.save_unsupported",
            "this backend does not persist settings",
        ))
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
            fn test_connection(
                &self,
                _: &SettingsDraft,
                _: Option<&str>,
            ) -> Result<ConnectionReport, BackendError> {
                Ok(ConnectionReport { latency_ms: 1 })
            }
            fn discover(
                &self,
                _: &SettingsDraft,
                _: Option<&str>,
            ) -> Result<Vec<UiModelEntry>, BackendError> {
                Ok(vec![])
            }
        }
        let backend = Minimal;
        assert!(backend.model_info("x").is_none());
        assert!(backend
            .preview_request_url(&SettingsDraft::default())
            .is_none());
        assert!(backend.load_settings().is_none());
        // 未实现保存的后端必须报错，**不能假装成功**（否则界面谎称已保存）
        assert!(backend
            .save_settings(&SettingsDraft::default(), "")
            .is_err());
    }

    #[test]
    fn saved_settings_never_carries_the_secret() {
        let saved = SavedSettings {
            provider: "deepseek".into(),
            protocol: "openai_chat".into(),
            endpoint: "https://api.deepseek.com/v1".into(),
            model_id: Some("deepseek-chat".into()),
            context_window: Some(128_000),
            has_api_key: true,
        };
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains("sk-"), "已保存配置里不该出现密钥形态");
        assert!(json.contains("has_api_key"));
    }
}
