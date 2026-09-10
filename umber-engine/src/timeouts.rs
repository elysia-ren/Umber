//! 四段超时（总案 §31.1）。
//!
//! 单一总超时会误杀推理模型（首 token 可达分钟级），因此正式细分为四段；
//! 全部属于 Runtime Core，Adapter 不得私有实现。

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TimeoutPolicy {
    /// 建立连接。
    pub connect: Duration,
    /// 请求发出 → 首个事件。推理模型可达分钟级，默认值必须宽松。
    pub first_token: Duration,
    /// 流中相邻事件的最大间隔（stall 检测）。
    pub idle: Duration,
    /// 整个 Invocation 的可选上限。
    pub total: Option<Duration>,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            first_token: Duration::from_secs(300),
            idle: Duration::from_secs(60),
            total: None,
        }
    }
}
