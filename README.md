# Universal Embedded Model Runtime

可嵌入式统一 AI 模型运行时。随宿主软件分发、直接嵌入宿主进程，
把协议差异 / 模型知识 / 流式行为 / 错误 / 凭据统一消化，
宿主只面对一套稳定的 Canonical API。

- 架构总案：`../Universal Embedded Model Runtime：V0 架构总案（修订版）.md`
- 开发计划：`../Universal Embedded Model Runtime：开发计划.md`
- 契约自审：`docs/CONTRACT_REVIEW.md`
- **宿主集成指南：`docs/HOST_INTEGRATION.md`**

铁律：**本仓库只有 `runtime-ffi` 允许 `unsafe`；其余 crate 一律 `#![forbid(unsafe_code)]`。**

## Crate 结构（总案 §53）

```text
model-runtime/
├── runtime-core/         Canonical API 契约（Request / Event / Response / Error / Invocation / Usage）
├── runtime-model/        Model Intelligence（Identity / Deployment / Capability / Evidence / Resolver / Catalog / Registry / Probe）
├── runtime-engine/       Invocation 引擎（终结保证 / 四段超时 / Retry / 部分结果 / 取消）
├── runtime-provider/     Provider Adapter trait（§41.4）
├── runtime-protocol/     四协议 Adapter + SSE/传输/错误映射基建
├── runtime-conformance/  Conformance 套件（fake-provider / 断言库 / fixture 格式）
├── runtime-credential/   CredentialStore 契约 + 内存实现 + 脱敏工具
├── runtime-ui/           UISpec 契约（设置 schema / 校验 / 发现状态机 / i18n）
├── runtime-ffi/          Stable C ABI（拉取式）+ C 宿主示例；唯一允许 unsafe 的 crate
└── catalog-builder/      数据供应链流水线（不随宿主分发）
```

## 进度

| 里程碑 | 状态 |
|--------|------|
| M0 契约冻结 | **完成**：七契约代码级落地 + ABI spike PASS（余：owner 签收后补 `contract-v1` 标记——本机 git 不可用） |
| M1 Core 骨架 + Conformance 基建 | **完成**：engine（终结保证/四段超时/Retry/部分结果）+ conformance + credential |
| M2 openai_chat Adapter | **完成**：9 conformance 用例 |
| M3 anthropic_messages Adapter | **完成**：8 用例（含 thinking 签名透传往返、缓存断点） |
| M4 openai_responses Adapter | **完成**：6 用例（含 encrypted_content 回传） |
| M5 gemini Adapter | **完成**：8 用例（含 safety→ContentFiltered、responseSchema 子集映射） |
| M6 Model Intelligence | **完成**：Registry 三层 + 字段级仲裁接入 + Catalog 加载 |
| M7 Probe | **完成**：Passive 白名单强制 / Active 显式开启 / 持久缓存与四类失效触发 |
| M8 Catalog Builder | **完成**：规范化 / 身份合并 / 冲突检测 / 许可证门禁 / CLI |
| M9 FFI 稳定化 | **形态已验证**（ABI spike PASS）；余：cbindgen 头文件生成、OS Keystore 实现、Python 绑定示例 |
| M10 Runtime UI | **UISpec 契约完成**（schema/校验/发现状态机/双语 i18n + key 全覆盖测试）；余：视觉参考实现（无 GUI 依赖，随宿主环境落地） |
| M11 集成与发布 | **文档与端到端验收完成**；余：三平台打包、真实网络冒烟 |

## 质量状态

```text
142 个测试全绿   |   cargo clippy -D warnings 零警告   |   cargo fmt 干净
```

各 crate 测试分布：core 16 · model 24 · engine 11 · protocol 43 · ui 10 ·
conformance 6 · credential 4 · ffi 5 · catalog-builder 3 · 端到端旅程 1 · 契约往返 1 …

## 已验证的关键保证（每条都有对应测试）

```text
终结事件保证        断流 / EOF / 畸形 / 取消 → 恰好一个终结事件（§23.1）
部分结果保证        失败或取消后 partial() 仍可取回内容与 usage（§25.1）
请求体格式          四协议各自形状 + 默认档位不冗余发送
签名透传            Anthropic thinking signature / Responses encrypted_content 原样往返（§19.1）
缓存断点            块级 cache_control → Anthropic system cache_control（§20）
错误映射            429/529/配额/上下文超长/安全拦截 → 14 类 ModelError（§28）
并发工具调用        按 index / call_id 归属重组（§24）
Discovery 回退      无 /models 不阻断，可手动添加 Model ID（§36）
字段级仲裁          行为/规格/价格/身份四表 + 用户覆盖最优先 + 5% 数字冲突阈值（§13）
Probe 门禁          Passive 只允许非生成性方法；Active 需显式开启（§17）
许可证门禁          白名单外的数据源直接拒绝构建（§62）
脱敏                敏感 header / JSON key 强制脱敏，SecretString 打码（§28 §32）
ABI                 C 宿主实测：握手 / 拉取 / WOULD_BLOCK / 所有权 / NULL 安全（§50）
端到端旅程          UISpec 配置 → 凭据 → 发现回退 → Endpoint 覆盖 → Registry → Adapter → Engine
```

## 本地开发

```text
cargo test                                  # 全量测试（142）
cargo clippy --all-targets -- -D warnings   # 零警告门禁
cargo fmt                                   # 格式化

# C ABI 示例（Windows / MSVC）
runtime-ffi\examples\build_spike.cmd debug
```

要求：Rust stable（edition 2021，MSRV 1.75）。CI 见 `.github/workflows/ci.yml`
（fmt / clippy -D warnings / test，三平台矩阵）。

## 待办（环境相关，非设计缺口）

1. **`contract-v1` 标记**：本机 GitHub 直连被重置、提权安装被拒，git 不可用；
   七契约自审已通过（`docs/CONTRACT_REVIEW.md`）。
2. **真实网络冒烟**：当前全部经 `ScriptedTransport`；每个协议需在联网环境对
   真实端点各做一次人工冒烟（计划中的"人工门禁"）。
3. **真实 HttpTransport**：TLS / 代理 / 读超时实现——接缝已就绪（`HttpTransport` trait）。
4. **覆盖率测量**：需 llvm-cov/tarpaulin 工具链（计划门槛 core ≥85%）。
