# Contract Review — 七大契约自审（M0）

对照总案 §41 逐项核对。**状态：七契约全部存在代码级定义；四协议 Adapter 与全部消费方已按契约实现；ABI spike 已通过；**`contract-v1` 已打标**。**

## §41.1 Canonical API

| 契约项 | 落点 | 状态 |
|--------|------|------|
| GenerateRequest（model=Deployment / tools / tool_choice / reasoning / generation / modalities / response_format / caching / metadata） | `runtime-core::request` | ✅ |
| Message + 七种统一内容块；Reasoning.provider_payload 透传 | `runtime-core::{message,content}` | ✅ |
| GenerateResponse + StopReason 五值 | `runtime-core::response` | ✅ |
| ModelEvent：append-only Delta + 全局单调 sequence + 终结事件恰好一个 | `runtime-core::event`（`SequenceValidator` 强制单调） | ✅ |
| Invocation：状态机 + partial() 部分结果 | `runtime-core::invocation` | ✅ |
| ModelError 14 类 + retryable 默认表 + raw_context 脱敏要求 | `runtime-core::error` | ✅ |
| Usage 统一字段 + provider_usage | `runtime-core::usage` | ✅ |

## §41.2 ModelInfo

| 契约项 | 落点 | 状态 |
|--------|------|------|
| ModelIdentity（规范化精确匹配，禁止模糊合并） | `runtime-model::identity` | ✅ |
| Endpoint / Deployment / ProtocolKind（Provider ≠ Protocol） | `runtime-model::deployment` | ✅ |
| ModelInfo 全字段；未记录能力 = Unknown | `runtime-model::model` | ✅ |
| Pricing：informational + effective_at | `runtime-model::model::Pricing` | ✅ |
| CompatibilityProfile：未记录特性不假定兼容 | `runtime-model::compatibility` | ✅ |
| Catalog format_version 兼容区间 | `runtime-model::catalog` | ✅ |

## §41.3 CapabilityRecord

`status / source / confidence(先验) / verified_at` — `runtime-model::capability` ✅

## §41.4 Provider Adapter Contract

`runtime-provider`：`ProviderAdapter { describe / discover_models / execute }`，
execute 返回 engine 的拉取式 `ProviderStream`。✅

## §41.5 Runtime UI Contract（UISpec）

M10 交付；本轮不动。

## §41.6 FFI / ABI Contract

**ABI spike PASS**（C 宿主 + MSVC 实测；头文件已由 cbindgen 从 Rust 类型生成，Python ctypes 绑定实测通过）：握手、拉取式 `runtime_stream_next`
（阻塞等待 + 超时 + WOULD_BLOCK）、所有权（string_free）、NULL 安全、
终结保证、全局 sequence、catch_unwind 全入口。每个 unsafe 入口带 `# Safety`
所有权与线程安全文档。✅

## §41.7 Versioning Contract

Runtime semver / Catalog 区间 / ABI major 握手 — 策略文档 + Catalog 校验已落地；ABI 握手随 spike 落地。

## 决议

1. §41.4、§41.6 补齐后，全部七契约存在代码级定义 → 打 `contract-v1`。
2. 已知偏差（记录在案，均不破坏铁律）：
   - 引擎侧 `ProviderStream` 由 Adapter 拉取、Engine 负责打 sequence 与终结合成 —— 与"Adapter 输出 Canonical Event"不矛盾，sequence 归属 Runtime（§24 全局单调由 Runtime 保证）。
   - connect/first_token 超时对阻塞式 open 采用"事后计量"（不可抢占），文档化于 `runtime-engine`。
