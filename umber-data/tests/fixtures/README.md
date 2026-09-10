# 测试夹具与数据诊断工具

## 夹具（`*_sample.json`）

从上游数据库**实测抓取并裁剪**的真实数据（保留真实字段结构，只截取少数条目），
让测试不依赖网络但验证真实形状。

抓取日期：2026-09-10（上游数据会变，夹具不会——这正是它作为回归基线的价值）

| 文件 | 上游 | 许可证 | 说明 |
|------|------|--------|------|
| `models_dev_sample.json` | [models.dev](https://github.com/sst/models.dev) | MIT | 2 个 provider 的真实条目（含 limit / cost / modalities） |
| `litellm_sample.json` | [BerriAI/litellm](https://github.com/BerriAI/litellm) | MIT | 真实条目（每 token 价格，含大小写重复键坑） |
| `openrouter_sample.json` | [OpenRouter](https://openrouter.ai/api/v1/models) | 见 OpenRouter 条款 | 真实条目（含 `reasoning.supported_efforts`） |

## 工具脚本

```text
extract_samples.py      从 snapshots/ 里的全量快照重新裁剪夹具
list_provider_keys.py   列出上游 provider 键（配置 ProviderPreset 的
                        catalog_provider_ids 时需要）
check_dups.py           检查 Canonical Catalog 里 canonical_id 是否重复
                        （身份合并的回归检查，必须是 0）
check_orgs.py           检查 provider 归属分布
check_names.py          检查显示名质量（是否有 provider 后缀/括号噪音）
check_catalog_ids.py    检查某几个模型是否在目录里（排查界面徽标缺失）
```

用法示例：

```bash
# 先取全量快照（不入版本库，见 .gitignore）
curl -o umber-data/snapshots/models_dev.json https://models.dev/api.json
curl -o umber-data/snapshots/litellm.json    https://cdn.jsdelivr.net/gh/BerriAI/litellm@main/model_prices_and_context_window.json

python umber-data/tests/fixtures/extract_samples.py
python umber-data/tests/fixtures/check_dups.py
```

## 关于 OpenRouter 数据

`umber-data` 把 OpenRouter 标为 **reference-only**（见 `licenses.rs`）：
公开 API ≠ 允许再分发。因此：

- 它**不进入**随包分发的 Bundled Catalog
- 这里保留样本仅用于测试适配器的解析正确性
- **实测后果**：唯一直接提供"每模型思考强度档位"的上游正是 OpenRouter，
  所以当前可再分发数据里 `supported_efforts` 为空 → Runtime 如实报告
  `unknown`，不做本地降级（§X.16）。这是正确行为，不是缺陷；
  要补齐该能力需与 OpenRouter 确认数据条款，或走官方覆盖层人工核对。

## 全量快照

`umber-data/snapshots/` 存全量快照供构建期使用，**不入版本库**
（约 7 MB 且可由脚本重建）。构建命令见 `docs/MODEL_DATA.md`。

实测结果（2026-09-10）：7619 + 3120 条输入 → **3233 个规范身份**，
961 条检出字段冲突，0 个重复 canonical_id。
