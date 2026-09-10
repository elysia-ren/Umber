# 契约对照表

V0 架构总案的 §41 定义了七大契约。本页把每条契约映射到实现它的代码位置——
**Rust 类型是唯一真值**，本页只是索引，不重复定义字段。

代码位置会随重构移动，以源码与 `cargo doc` 为准。

## §41.1 Canonical API — `runtime-core`

| 契约项 | 落点 |
|--------|------|
| GenerateRequest（model=Deployment / tools / tool_choice / reasoning / generation / modalities / response_format / caching / metadata） | `runtime-core::request` |
| Message + 七种统一内容块；Reasoning.provider_payload 透传 | `runtime-core::{message, content}` |
| GenerateResponse + StopReason 五值 | `runtime-core::response` |
| ModelEvent：append-only Delta + 全局单调 sequence + 终结事件恰好一个 | `runtime-core::event`（`SequenceValidator` 强制单调） |
| Invocation：状态机 + partial() 部分结果 | `runtime-core::invocation` |
| ModelError 14 类 + retryable 默认表 + raw_context 脱敏要求 | `runtime-core::error` |
| Usage 统一字段 + provider_usage | `runtime-core::usage` |

## §41.2 ModelInfo — `runtime-model`

| 契约项 | 落点 |
|--------|------|
| ModelIdentity（规范化精确匹配，禁止模糊合并） | `runtime-model::identity` |
| Endpoint / Deployment / ProtocolKind（Provider ≠ Protocol） | `runtime-model::deployment` |
| ModelInfo 全字段；未记录能力 = Unknown | `runtime-model::model` |
| Pricing：informational + effective_at | `runtime-model::model::Pricing` |
| CompatibilityProfile：未记录特性不假定兼容 | `runtime-model::compatibility` |
| Catalog format_version 兼容区间 | `runtime-model::catalog` |

## §41.3 CapabilityRecord — `runtime-model::capability`

`status` / `source` / `confidence`（先验）/ `verified_at`。

## §41.4 Provider Adapter Contract — `runtime-provider`

`ProviderAdapter { describe / discover_models / execute }`；`execute` 返回 Engine 的
拉取式 `ProviderStream`。

## §41.5 Runtime UI Contract（UISpec） — `runtime-ui`

设置 schema / 校验 / 发现状态机 / i18n。参考实现在 `runtime-ui-egui`：
**视觉层不是契约**，只要满足 UISpec 数据契约即可整体替换。

## §41.6 FFI / ABI Contract — `runtime-ffi`

C ABI 自 0.2 起走真实链路：`runtime-ffi` 依赖 `runtime-protocol` /
`runtime-credential` / `runtime-model`，宿主经 `runtime_set_deployment` +
`runtime_set_credential`（可选 `runtime_load_catalog`）即可走四协议 Adapter 与
真实 HTTP/SSE，与 Rust 宿主同一条链路。内置 demo 假流改为**显式开启**
（`runtime_set_demo`）：未配置时返回 `UMER_ERR_NOT_CONFIGURED`，不静默返回假数据。

已覆盖：握手、拉取式 `runtime_stream_next`（阻塞等待 + 超时 + WOULD_BLOCK）、
所有权（`string_free`）、NULL 安全、终结保证、全局 sequence、全部入口 catch_unwind，
以及「C ABI → 真实 Adapter → ScriptedTransport → Engine」端到端回归。
头文件由 cbindgen 从 Rust 类型生成（勿手改）；Python ctypes 绑定暴露同一组配置入口；
每个 unsafe 入口都带 `# Safety` 文档段。

## §41.7 Versioning Contract

Runtime semver / Catalog `format_version` 兼容区间 / ABI major 握手。

## 已知偏差（记录在案）

- 引擎侧 `ProviderStream` 由 Adapter 拉取、Engine 负责签发 sequence 与终结合成：
  与"Adapter 输出 Canonical Event"不矛盾——sequence 归属 Runtime（§24 全局单调
  由 Runtime 保证）。
- connect / first_token 超时对阻塞式 `open` 采用"事后计量"（不可抢占），
  文档化于 `runtime-engine`。
