"""更新 MODEL_DATA.md：记录身份合并修复与 deployments 表。"""
import io

P = r"C:\个人文件\API\model-runtime\docs\MODEL_DATA.md"
c = io.open(P, encoding="utf-8").read()

old = """```text
models.dev    7619 条
LiteLLM       3120 条
OpenRouter     436 条（reference-only，不进随包数据）
────────────────────────
合并输出      6158 个规范模型，944 条检出字段冲突
```"""
new = """```text
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
嵌套括号"分级，同级取最短**——得到 `DeepSeek R1` 这类干净名字。"""
assert old in c, "stats block not found"
c = c.replace(old, new)

# 补一段：UI 与数据体系的关系
c += """

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
"""
io.open(P, "w", encoding="utf-8", newline="\n").write(c)
print("MODEL_DATA.md updated")
