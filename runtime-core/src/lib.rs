//! Universal Embedded Model Runtime — Canonical API 契约类型。
//!
//! 本 crate 是总案 §41.1 Canonical API Contract 的唯一真值：
//! 所有 Provider 最终必须映射到这里，Provider 类型不得出现在本 crate。
//!
//! 铁律速查（总案 §67）：
//! - Delta 一律 append-only 且全局单调（§23 §24）
//! - 每个 Invocation 恰好一个终结事件（§23.1）
//! - 失败 / 取消必可取回部分结果（§25.1）
//! - `stop_reason` 是宿主判断循环的唯一依据（§27）
//! - `Reasoning.provider_payload` 原样透传（§19.1）

#![forbid(unsafe_code)]

pub mod content;
pub mod error;
pub mod event;
pub mod ids;
pub mod invocation;
pub mod message;
pub mod request;
pub mod response;
pub mod usage;

pub use content::ContentBlock;
pub use error::ModelError;
pub use event::{ModelEvent, SequenceValidator, SequencedEvent};
pub use ids::{BlockId, CallId, DeploymentId, InvocationId};
pub use invocation::{InvalidTransition, Invocation, InvocationState, PartialOutput};
pub use message::{Message, Role};
pub use request::{
    CacheMode, CachingConfig, GenerateRequest, GenerationConfig, OutputModalities, ReasoningConfig,
    ReasoningEffort, RequestValidationError, ResponseFormat, Tool, ToolChoice,
};
pub use response::{GenerateResponse, ProviderContext, StopReason};
pub use usage::Usage;
