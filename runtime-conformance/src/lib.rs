//! Conformance Suite（总案 §52）。
//!
//! 本 crate 是正式交付物：每个协议 Adapter 必须通过对应 Conformance
//! 才能进入主干——这是唯一门票。
//!
//! - `fake`：可脚本化 fake provider（延迟 / 断流 / 畸形 / 错误注入）
//! - `assert`：事件流断言（单调 / 唯一终结 / Delta 重组）
//! - `fixture`：golden request/response 序列文件格式

#![forbid(unsafe_code)]

pub mod assert;
pub mod fake;
pub mod fixture;

pub use fixture::{Expectation, Fixture};
