//! Invocation 引擎（总案 §23–§32）。
//!
//! 职责（全部属于 Runtime Core，Adapter 不得私有实现，§28–§31）：
//! - 终结事件保证：无论底层发生什么，恰好产生一个 Completed / Failed / Cancelled（§23.1）
//! - 全局单调 sequence 由 Engine 统一签发（§24）
//! - 四段超时：connect / first_token / idle / total（§31.1）
//! - Retry：安全重放判断 + Retry-After（§29 §30）
//! - 部分结果：失败 / 取消后 partial() 永远可取回（§25.1）
//!
//! 已知限制（记录在案）：阻塞式 `SourceFactory::open` 不可抢占，
//! connect 超时采用事后计量；事件粒度同理，total 超时在事件间隙检查。

#![forbid(unsafe_code)]

pub mod assembler;
pub mod cancel;
pub mod engine;
pub mod retry;
pub mod source;
pub mod timeouts;

pub use assembler::StreamAssembler;
pub use cancel::CancelToken;
pub use engine::{run_invocation, InvocationOutcome};
pub use retry::{decide, RetryDecision, RetryPolicy};
pub use source::{ProviderStream, SourceFactory};
pub use timeouts::TimeoutPolicy;
