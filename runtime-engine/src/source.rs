//! Adapter 侧事件源契约（总案 §39.4 execute 的产物）。

use std::time::Instant;

use runtime_core::{ModelError, ModelEvent};

/// 拉取式事件源。Engine 阻塞调用 `next_event` 直到超时边界。
///
/// 契约：
/// - 不得发送 `Started`（由 Engine 合成；sequence 归 Engine 签发，§24）
/// - `Completed` / `Failed` 只能作为最后一个事件（终结事件保证，§23.1）
/// - 不得阻塞超过 `deadline`；超时返回 `Err(Timeout)`（kind 由 Engine 重标，§31.1）
/// - `Ok(None)` 表示适配器侧流正常结束——若未发终结事件，
///   Engine 会合成 `Failed`（§23.1）
pub trait ProviderStream: Send {
    fn next_event(&mut self, deadline: Instant) -> Result<Option<ModelEvent>, ModelError>;
}

/// 事件源工厂。Retry 触发安全重放时，Engine 通过它重新打开事件源（总案 §29）。
pub trait SourceFactory: Send + Sync {
    fn open(&self) -> Result<Box<dyn ProviderStream>, ModelError>;
}

impl<F> SourceFactory for F
where
    F: Fn() -> Result<Box<dyn ProviderStream>, ModelError> + Send + Sync,
{
    fn open(&self) -> Result<Box<dyn ProviderStream>, ModelError> {
        self()
    }
}
