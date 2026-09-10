# Universal Embedded Model Runtime

可嵌入式统一 AI 模型运行时。随宿主软件分发、直接嵌入宿主进程，
把协议差异 / 模型知识 / 流式行为 / 错误 / 凭据统一消化，
宿主只面对一套稳定的 Canonical API。

- 架构总案：[`docs/architecture/V0-架构总案.md`](docs/architecture/V0-架构总案.md)（初版存于 [`docs/architecture/V0-架构总案-初版.md`](docs/architecture/V0-架构总案-初版.md)）
- 开发计划：[`docs/architecture/开发计划.md`](docs/architecture/开发计划.md)
- 契约自审：`docs/CONTRACT_REVIEW.md`
- **宿主集成指南：`docs/HOST_INTEGRATION.md`**

铁律：**本仓库只有 `runtime-ffi` 允许 `unsafe`；其余 crate 一律 `#![forbid(unsafe_code)]`。**

## Crate 结构（总案 §53）

```text
model-runtime/
├── runtime-core/          Canonical API 契约（Request / Event / Response / Error / Invocation / Usage）
├── runtime-model/         Model Intelligence（Identity / Deployment / Capability / Evidence / Resolver / Catalog / Registry / Probe）
├── runtime-engine/        Invocation 引擎（终结保证 / 四段超时 / Retry / 部分结果 / 取消）
├── runtime-provider/      Provider Adapter trait（§41.4）
├── runtime-protocol/      四协议 Adapter + 真实 HTTP 传输 + SSE/错误映射基建
├── runtime-conformance/   Conformance 套件（fake-provider / 断言库 / fixture 格式）
├── runtime-credential/    CredentialStore 契约 + 内存实现 + 脱敏工具
├── runtime-credential-os/ 平台凭据（系统钥匙串 / 加密文件 / 回退链）
├── runtime-ui/            UISpec 契约（设置 schema / 校验 / 发现状态机 / i18n）
├── runtime-ffi/           Stable C ABI（拉取式）+ C/Python 绑定；唯一允许 unsafe 的 crate
├── runtime-data/          Model Data Pipeline & Database（上游适配器 / 规范化 / 冲突解析 / 本地库）
└── runtime-ffi 之外的数据侧工具见 runtime-data 的 `model-data` CLI
```

## 进度

**M0–M11 全部完成**（`contract-v1` 已打标）。

| 里程碑 | 状态 |
|--------|------|
| M0 契约冻结 | 完成：七契约代码级落地 + ABI spike PASS |
| M1 Core + Conformance 基建 | 完成：终结保证 / 四段超时 / Retry / 部分结果 |
| M2 openai_chat | 完成：9 用例 + **真实 DeepSeek 生成、工具调用、取消实测** |
| M3 anthropic_messages | 完成：8 用例（thinking 签名透传、缓存断点） |
| M4 openai_responses | 完成：6 用例（encrypted_content 回传） |
| M5 gemini | 完成：8 用例（safety→ContentFiltered、responseSchema 子集映射） |
| M6 Model Intelligence | 完成：Registry 三层 + 字段级仲裁 + Catalog 加载 |
| M7 Probe | 完成：Passive 白名单强制 / Active 显式开启 / 四类失效触发 |
| M8 Catalog Builder | 完成：规范化 / 身份合并 / 冲突检测 / 许可证门禁 / CLI |
| M9 FFI 稳定化 | 完成：C ABI + **系统代理发现** + **OS 凭据三档回退** + cbindgen 生成头文件 + Python ctypes 绑定 + C 宿主示例 |
| M10 Runtime UI | **完成**：UISpec 数据契约 + egui/eframe 参考实现（侧栏厂商列表 + 内容区、30 个厂商预置分四类、模型数据呈现、证据行、无头测试、截图脚本） |
| M11 集成与发布 | 完成：宿主集成指南 + 端到端验收 + `contract-v1` 标记 |

## 质量状态

```text
155 个测试通过（154 pass + 1 ignored 写真实钥匙串）
cargo clippy -D warnings 零警告   |   cargo fmt 干净
真实网络验证：8/8 通过（含 DeepSeek 真实流式生成、工具调用、中途取消）
设置窗口实测（release）：exe 6.7 MB，窗口正常启动渲染（RSS ~120 MB，见 UI 选型节）
```

## UI 技术选型（egui/eframe，已实施）

选型约束：体积小 / 内存小 / 性能好 / 现代观感 / **零商用风险**。
排除 Electron（用户明确要求）；排除 Slint（免版税档的归属义务与
"不得暴露 Slint API"条款同嵌入型组件冲突，embedded 设备不在覆盖内）。

`runtime-ui-egui` 是**参考实现，可替换**（视觉不是契约，§38）：

- 只依赖 `runtime-ui` 数据契约，不碰网络 / 文件 / 凭据——动作全走 `SettingsBackend`
- 表单由 schema 驱动渲染：契约加字段，UI 代码零改动跟随
- 明暗双主题 token + Compact/Cozy 密度 + `pixels_per_point` 缩放（§38 四渲染参数）
- 中文回退字体从系统加载（微软雅黑 / 苹方 / Noto CJK），不打包字体（省 10–20 MB）
- 无头帧测试：不开窗口即可验证完整渲染路径
- 实测（Windows / release）：exe 6.7 MB；RSS ~120 MB，主要来自 glow/GL 与 winit。
  待调优项：裁剪 accesskit、按需重绘（`request_repaint` 节流）、必要时评估
  iced+tiny-skia 纯软渲染路线；**是调优空间，不是选型缺陷**
- 思考强度控件已进 schema（`default_reasoning_effort`，§21.1 四档位）；
  就近降级与生效档位回写仍待实现（见"已知缺口"）


## 真实网络验证结果

```text
DeepSeek 流式生成     24 事件 / EndTurn / usage 11→19 / 真实回复文本        ✅
DeepSeek 工具调用     tool_choice=Required → get_time {"city":"杭州"}        ✅
DeepSeek 中途取消     4 事件后 Cancelled，取回部分内容 + 部分结果保证成立    ✅
四家官方端点可达      DeepSeek / OpenAI(Chat+Responses) / Anthropic / Gemini ✅
系统代理发现          Windows 系统代理（127.0.0.1:7897）自动生效（否则连不上）✅
Windows 钥匙串        写入 → 读取 → 删除 真实往返                            ✅
C 宿主（MSVC）        生成头文件下依然 PASS                                   ✅
Python ctypes 绑定    7 事件 + 终结事件 + 所有权正确（无堆损坏）              ✅
```

复现：

```text
cargo test -p runtime-protocol --test network_smoke -- --ignored --nocapture
cargo test -p runtime-credential-os os_keystore_round_trip -- --ignored
runtime-ffi\examples\build_spike.cmd debug
python runtime-ffi\bindings\python\umer.py
```

## 已验证的关键保证（每条都有对应测试）

```text
终结事件保证        断流 / EOF / 畸形 / 取消 → 恰好一个终结事件（§23.1）
部分结果保证        失败或取消后 partial() 仍可取回内容与 usage（§25.1）— 真实网络下已验证
请求体格式          四协议各自形状 + 默认档位不冗余发送
签名透传            Anthropic thinking signature / Responses encrypted_content 原样往返（§19.1）
缓存断点            块级 cache_control → Anthropic system cache_control（§20）
错误映射            429/529/配额/上下文超长/安全拦截 → 14 类 ModelError（§28）
并发工具调用        按 index / call_id 归属重组（§24）— 真实网络下已验证
Discovery 回退      无 /models 不阻断，可手动添加 Model ID（§36）
字段级仲裁          行为/规格/价格/身份四表 + 用户覆盖最优先 + 5% 数字冲突阈值（§13）
Probe 门禁          Passive 只允许非生成性方法；Active 需显式开启（§17）
许可证门禁          白名单外的数据源直接拒绝构建（§62）
脱敏                敏感 header / JSON key 强制脱敏，SecretString 打码（§28 §32）
凭据回退链          宿主 → 系统钥匙串 → 加密文件；落到加密文件时强制告警（§32.1）
加密文件存储        ChaCha20-Poly1305，明文不落盘，错密钥必须报错（测试强制）
ABI                 C 宿主 + Python 绑定实测：握手 / 拉取 / WOULD_BLOCK / 所有权 / NULL 安全（§50）
端到端旅程          UISpec 配置 → 凭据 → 发现回退 → Endpoint 覆盖 → Registry → Adapter → Engine
```

## 真实世界踩到的三个问题（fixtures 抓不到，已修复）

1. **纯文本错误体**：DeepSeek 无凭据返回 `Authentication Fails (governor)`（非 JSON）。
   传输层原先要求 JSON 并报 Unknown 错误 → 改为返回 `HttpResponse::Text`，
   由协议层统一映射为 `AuthenticationFailed`。
2. **系统代理**：用户在 Windows"Internet 选项"里开代理不会设环境变量，
   而 ureq 只读环境变量 → 传输层新增系统代理发现（Windows 注册表 / macOS `scutil`）。
   不修这条，宿主在用户机器上会莫名连不上。
3. **ctypes 所有权陷阱**：`c_char_p` 字段访问会解引用成 Python bytes，
   交给 `runtime_string_free` 等于 free 掉 Python 自己的缓冲区（实测堆损坏 `0xC0000374`）
   → 绑定层改用 `c_void_p` 保持原始指针。

## 本地开发

```text
cargo test                                  # 全量测试
cargo clippy --all-targets -- -D warnings   # 零警告门禁
cargo fmt                                   # 格式化

# C ABI 示例（Windows / MSVC）
runtime-ffi\examples\build_spike.cmd debug

# 重新生成 C 头文件（Rust 类型是唯一真值）
cbindgen --config runtime-ffi/cbindgen.toml --crate runtime-ffi -o runtime-ffi/include/umer.h
```

要求：Rust stable（edition 2021，MSRV 1.75）。CI 见 `.github/workflows/ci.yml`
（fmt / clippy -D warnings / test，三平台矩阵）。

## 版本标记

```text
contract-v1    七大契约冻结（Canonical API / ModelInfo / CapabilityRecord /
               Provider Adapter / UISpec / FFI-ABI / Versioning）
```

## 未做的与原因（非设计缺口）

```text
真实冒烟需要凭据   四协议的"真实生成"冒烟已就位（network_smoke.rs），
                  通过环境变量 UMER_SMOKE_*_KEY 启用；CI 永不消耗真实 Token
覆盖率测量         需 llvm-cov/tarpaulin 工具链，未在计划门槛内强制
参考 UI 视觉实现   UISpec 数据契约已冻结；视觉层随宿主环境落地（契约特意不依赖 GUI 库）
```

