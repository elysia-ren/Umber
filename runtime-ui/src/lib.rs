//! Runtime UI — UISpec 数据契约（总案 §38 §41.5）。
//!
//! 正式决定：**冻结的是 UISpec 数据契约；视觉与控件实现不是契约。**
//!
//! - Core 暴露纯数据接口（settings_schema / apply / test_connection /
//!   discovery_session / strings），宿主用任何技术渲染
//! - 配置与状态的全部逻辑属于 Core；宿主渲染的只是 UISpec
//! - 参考实现（视觉层）可替换，随宿主 Theme / Language / Scale / Density
//!   走渲染参数，不影响契约
//!
//! 本 crate 不依赖任何 GUI 库——这是契约可冻结的前提。

#![forbid(unsafe_code)]

pub mod backend;
pub mod discovery;
pub mod schema;
pub mod strings;

pub use backend::{
    BackendError, ConnectionReport, ConnectionTestState, SettingsBackend, UiModelInfo,
};
// 能力类型随 UiModelInfo 一起再导出：UI 层只依赖 runtime-ui 一个契约面
pub use discovery::{DiscoverySession, DiscoveryState, UiModelEntry};
pub use runtime_model::capability::{CapabilityKind, CapabilityStatus};
pub use schema::{FieldKind, FieldSpec, SectionSpec, SettingsDraft, SettingsPage, ValidationIssue};
pub use strings::{Strings, BUILTIN_STRINGS_EN, BUILTIN_STRINGS_ZH};
