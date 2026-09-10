# 模型数据体系（规格 X 的实施说明）

本仓库按规格 X「Model Data System」实现了**整条数据管线**，代码在
`runtime-data`（管线与数据库）与 `runtime-model`（知识契约）。

## 一、分层与数据流

```text
外部成熟数据库（不重复造世界级模型数据库）
    Models.dev · LiteLLM · OpenRouter · 官方覆盖层
        ↓ runtime-data/src/sources/   每个上游一个 Source Adapter
    RawModelRecord（统一中间结构）
        ↓ runtime-data/src/pipeline.rs
    Normalize → Identity Match → Conflict Resolve → License Gate
        ↓
    Canonical Catalog（构建期产物，随 Runtime 分发）
        ↓ runtime-data/src/store.rs
    Runtime Local DB（用户机器上那一层）
        ↑ Deployment · Probe 结果 · User Override
        ↓ Evidence Resolution
    Effective Model Profile / ResolvedModel
```

对应规格的三类数据库：

| 规格 | 本仓库落点 |
|------|-----------|
| A. External Source Database | `runtime-data/src/sources/{models_dev,litellm,openrouter,official}.rs` |
| B. Build-time Canonical DB | `catalog-builder` → `runtime-data::pipeline::build` 产出 `Catalog` |
| C. Runtime Local DB | `runtime-data::store::LocalDb`（逐记录版本 + 覆盖 + 探测结果） |

## 二、实测能力（2026-09-10 真实数据）

```text
models.dev    7619 条
LiteLLM       3120 条
OpenRouter     436 条（reference-only，不进随包数据）
────────────────────────
合并输出      3233 个规范身份，961 条检出字段冲突
```

### 身份合并的正确性（修过一个真实的数据腐化 bug）

早期版本按**完整 source key**（`provider/model`）分组，但 `canonical_id`
只取裸模型名，于是 `302ai/glm-4.6` 与 `novita/glm-4.6` 各自成组、
各产出一条 `glm-4.6` —— 3233 个身份产出 6158 条记录，**1014 个 id 重复**。
下游按 id 索引会互相覆盖（界面上表现为"某厂商只剩一个模型"）。

按规格 X.1/X.18 修正：

```text
身份（Identity）  按裸模型名合并：同一个模型经不同 Gateway 暴露仍是同一身份
归属（Deployment）另存一张表：provider → model_id，不丢信息
```

回归测试钉住两条不变量：`canonical_id` 在 Catalog 内唯一；
同一模型的多个 provider 归属都保留在 `deployments` 里且可按 provider 查询。

标签页效果：`glm-4.6` 合并了 35 条来源证据；`deepseek` 的归属表有 9 个模型
（含官方的 `deepseek-chat` / `deepseek-reasoner` / `deepseek-v4-pro`），
智谱 15、Kimi 4、阿里 55、OpenAI 130。

### 显示名择优

上游名字常带 provider 后缀与括号注释（`Pro/deepseek-ai/DeepSeek-R1`、
`DeepSeek V3.2 (Vertex AI (OpenAI-compatible))`）。合并时取最长会把噪音
挑给用户，取最短又可能丢版本号。现在的规则是：**按"是否含路径 / 括号注释 /
嵌套括号"分级，同级取最短**——得到 `DeepSeek R1` 这类干净名字。

运行：

```bash
model-data build runtime-data/snapshots/{models_dev,litellm}.json -o runtime-data/out/catalog.json
model-data inspect runtime-data/out/catalog.json
model-data licenses
```

## 三、上游适配器实测到的坑（都已处理并有测试钉住）

1. **单位不统一**：models.dev 的 `cost` 是每百万 token，LiteLLM 与
   OpenRouter 是**每 token**（且 OpenRouter 用字符串）。适配器各自换算，
   测试用真实值断言（`6.2e-07 → $0.62/M`）。
2. **大小写重复键**：LiteLLM 文件里真的存在 `baai/...` 与 `BAAI/...`
   两个键。大小写不敏感的解析器会直接崩，Rust 的 map 是敏感的，两者都保留。
3. **档位词不统一**：OpenRouter 用 `max/high/low`，规范是
   `minimal/low/medium/high`。归一 + **就近降级**在 `runtime-model::effort`。
4. **不是所有错误体都是 JSON**：DeepSeek 无凭据返回纯文本
   `Authentication Fails (governor)`（这属于传输层，见 `runtime-protocol`）。

## 四、两条必须记住的事实

**1. 唯一提供"每模型思考强度档位"的上游是 OpenRouter，而它是 reference-only。**
公开 API ≠ 允许再分发，因此它不进随包 Catalog。后果是可再分发数据里
`supported_efforts` 为空，Runtime 如实报告 `unknown` 且**不做本地降级**
（不猜，规格 X.16）。要补齐这项能力，需要与 OpenRouter 确认数据条款，
或走官方覆盖层人工核对。

**2. 冲突是常态，不是异常。** 6158 个模型里有 944 条存在字段级冲突，
`RecordMeta::has_conflict` 会标出来，两种来源的值都保留在 `evidence` 里。
规格 X.9 明确要求"不偷偷把冲突抹掉"，实现遵循了这一点。

## 五、许可与来源（规格 X.10）

`runtime-data/src/licenses.rs` 是白名单门禁：

- **可随包分发**：MIT / Apache-2.0 / CC0-1.0 / CC-BY-4.0 / CC-BY-SA-4.0 / BSD-3-Clause
- **仅构建参考**：Proprietary、CC-BY-NC-4.0、未审来源

`BuildOutput.reference_only_sources` 记录被排除的来源——排除有痕，不是静默丢弃。

## 六、存储选型的说明

逻辑表结构与规格 §5 一致，物理形态是「一表一 JSON 文件 + 原子写」而非 SQLite：

- 宿主体积是硬约束，捆绑 SQLite 引擎（C 代码）会显著增大产物
- 本地数据量级是**每个用户几十条记录**，不是百万行，索引没有价值
- 无 C 依赖 → 三平台构建一致

将来数据量或并发上升时，`LocalDb` 的读/写接口可以换成 SQLite，逻辑表结构不变。


## 七、这套数据体系在界面上的体现

设置界面不显示"一个模型列表"就完事，而是把解析链露出来：

```text
模型列表   DeepSeek Chat      [1M] [工具] [并行] [结构化]
           DeepSeek V4 Pro    [1M] [工具] [并行] [结构化]
           deepseek-coder     [128K] [工具]

模型知识   DeepSeek Chat
           能力   ToolCall·支持
           价格   输入 $0.1400/M · 输出 $0.2800/M（仅供参考）
           思考强度 未知（该服务未提供档位信息）
           上下文窗口 [留空使用自动探测值]
           目录 128,000 · 生效 128,000      ← Evidence/Resolver 的输出
```

三条界面纪律：

1. **推荐模型查表，不硬编码**：切厂商时向 Core 要该 provider 的模型
   （`Catalog::models_of_provider`），而不是读预置里的写死名单——
   预置里写死名单正是"界面显示一年前模型"的根因。
2. **刷新走服务商 API**：`刷新模型列表` 调 Adapter 的 `discover_models`
   （GET /models）；无密钥时会明说"需先填写 API Key"，失败时显示原因，
   **不静默吞掉**。
3. **未知就说未知**：能力未记录显示"目录未记录该能力字段"，
   档位无数据显示"该服务未提供档位信息"，绝不编造。
