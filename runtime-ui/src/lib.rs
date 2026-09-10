//! UISpec 数据类型（总案 §38 §41.5）。
//!
//! **冻结的是 UISpec 数据契约；视觉与控件实现不是契约。**
//!
//! 本 crate 不依赖任何 GUI 库——这是契约可冻结、且宿主可用任何技术渲染的前提。
//!
//! ```text
//! preset          厂商预置（几十个，分四类）
//! settings_state  设置界面状态（搜索/选择/自动带出/证据解析）
//! backend         UI ↔ Core 的动作接口
//! schema          声明式设置页 schema（高级选项区用）
//! strings         i18n 文案（key 全覆盖由测试强制）
//! discovery       模型发现状态机
//! ```

#![forbid(unsafe_code)]

pub mod backend;
pub mod discovery;
pub mod preset;
pub mod schema;
pub mod settings_state;
pub mod strings;

pub use backend::{
    BackendError, ConnectionReport, ConnectionTestState, SaveReport, SavedSettings,
    SettingsBackend, UiModelInfo,
};
// 能力与协议类型随数据类型一起再导出：UI 层只依赖 runtime-ui 一个契约面
pub use discovery::{DiscoverySession, DiscoveryState, UiModelEntry};
pub use preset::{
    preset_by_id, presets_by_category, search_presets, ProviderCategory, ProviderOffering,
    ProviderPreset, BUILTIN_PRESETS,
};
pub use runtime_model::capability::{CapabilityKind, CapabilityStatus};
pub use runtime_model::deployment::ProtocolKind;
pub use schema::{FieldKind, FieldSpec, SectionSpec, SettingsDraft, SettingsPage, ValidationIssue};
pub use settings_state::{
    protocol_slug, resolve_context_window, ContextWindowEvidence, ModelEntry, ModelSource,
    SettingsState,
};
pub use strings::{Strings, BUILTIN_STRINGS_EN, BUILTIN_STRINGS_ZH};
