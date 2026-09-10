//! Model Data Pipeline & Database（规格 X）。
//!
//! 本 crate 是**整条模型数据管线**，不只是 Catalog Builder：
//!
//! ```text
//!   外部成熟数据库                     ← 不重复造世界级模型数据库
//!   Models.dev / LiteLLM / OpenRouter / 官方
//!          ↓ sources（每个上游一个适配器）
//!   RawModelRecord（统一中间结构）
//!          ↓ pipeline
//!   Normalize → Identity Match → Conflict Resolve → License Gate
//!          ↓
//!   Canonical Model Catalog           ← 构建期产出，随 Runtime 分发
//!          ↓ store
//!   Runtime Local DB                  ← 用户机器上的那一层
//!          ↑ Deployment / Probe / User Override
//!          ↓ Evidence Resolution
//!   Effective Model Profile / ResolvedModel
//! ```
//!
//! 五条铁律（规格 X.28）在本 crate 的落点：
//!
//! ```text
//! 1. Model ID ≠ Model Identity      → record.rs 的 bare_model_id / identity 归一
//! 2. Identity ≠ Deployment          → profile.deployment 可空；Deployment 单独成表
//! 3. Catalog ≠ Runtime Truth        → store.rs 的 Local DB 叠加 Discovery/Probe/Override
//! 4. Unknown ≠ Unsupported          → 所有 `Option` 保持 None，绝不填 false/0
//! 5. External Catalog ≠ Dependency  → 上游只在构建期访问，产物随包分发
//! 6. 每个重要属性必须有 Evidence     → pipeline 逐字段保留来源，冲突可见
//! ```

#![forbid(unsafe_code)]

pub mod licenses;
pub mod pipeline;
pub mod record;
pub mod sources;
pub mod store;

pub use licenses::{LicenseDecision, SourceLicense};
pub use pipeline::{build, BuildOutput, RecordMeta};
pub use record::{RawModelRecord, RawPricing};
pub use sources::{
    LitellmAdapter, ModelsDevAdapter, OfficialOverlay, OpenRouterAdapter, SourceAdapter,
    SourceError,
};
pub use store::{LocalDb, StoreError, StoredDeployment, UserOverride, VersionedRecord};
