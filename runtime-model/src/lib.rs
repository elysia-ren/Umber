//! Universal Embedded Model Runtime — Model Intelligence 契约类型。
//!
//! 本 crate 覆盖总案 §41.2 ModelInfo Contract 与 §41.3 CapabilityRecord
//! Contract，以及 §13 的字段级数据优先级仲裁。
//!
//! 铁律速查（总案 §67）：
//! - ModelIdentity ≠ Deployment
//! - Capability ≠ Boolean
//! - 数据仲裁 = 字段级优先级，不是线性覆盖
//! - Identity Matching = 人工审核精确匹配，不是模糊合并
//! - Pricing 是 informational 数据，不构成计费依据（§46）

#![forbid(unsafe_code)]

pub mod capability;
pub mod catalog;
pub mod compatibility;
pub mod deployment;
pub mod evidence;
pub mod identity;
pub mod model;
pub mod probe;
pub mod registry;
pub mod resolver;

pub use capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
pub use catalog::{Catalog, CatalogFormatError, CatalogSource};
pub use compatibility::{CompatibilityFeature, CompatibilityLevel, CompatibilityProfile};
pub use deployment::{Deployment, Endpoint, ProtocolKind};
pub use evidence::{Evidence, EvidenceSource};
pub use identity::{normalize_model_id, ModelIdentity};
pub use model::{
    Modality, ModelInfo, ModelLimits, ModelModalities, Pricing, ReasoningInfo,
    StructuredOutputInfo, ToolSupport,
};
pub use probe::{
    ActiveProbeGuard, PassiveProbe, ProbeError, ProbeOutcome, ProbeResult, ProbeResultStore,
    ProbeTestType,
};
pub use registry::ModelRegistry;
pub use resolver::{
    resolve, FieldCandidate, FieldCategory, FieldValue, Resolution, NUMERIC_CONFLICT_TOLERANCE,
};
