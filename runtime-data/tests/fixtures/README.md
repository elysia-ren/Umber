# 测试夹具出处与许可

本目录下的 `*_sample.json` 是从上游数据库**实测抓取并裁剪**的真实数据
（保留真实字段结构，只截取少数条目），用于让测试不依赖网络但验证真实形状。

抓取日期：2026-09-10
抓取方式：`curl` 直连上游公开端点（本机经系统代理）

| 文件 | 上游 | 许可证 | 说明 |
|------|------|--------|------|
| `models_dev_sample.json` | [models.dev](https://github.com/sst/models.dev) | MIT | 2 个 provider 的真实条目（含 limit / cost / modalities） |
| `litellm_sample.json` | [BerriAI/litellm](https://github.com/BerriAI/litellm) | MIT | 真实条目（每 token 价格，含大小写重复键坑） |
| `openrouter_sample.json` | [OpenRouter](https://openrouter.ai/api/v1/models) | 见 OpenRouter 条款 | 真实条目（含 `reasoning.supported_efforts`） |

`extract_samples.py` 是抓取裁剪脚本，保留以便以后重新生成夹具。
`rename_modelinfo.py` 是一次性重构脚本（ModelInfo → ModelProfile），已完成使命。

## 关于 OpenRouter 数据

`runtime-data` 把 OpenRouter 标为 **reference-only**（见 `licenses.rs`）：
公开 API ≠ 允许再分发。因此：

- 它**不进入**随包分发的 Bundled Catalog
- 但本目录保留样本用于测试适配器的解析正确性（测试夹具属于合理使用）
- **实测后果**：唯一直接提供"每模型思考强度档位"的上游正是 OpenRouter，
  所以当前可再分发数据里 `supported_efforts` 为空 → Runtime 如实报告
  `unknown`，不做本地降级（§X.16）。这是正确的行为，不是缺陷；
  要补齐该能力需要与 OpenRouter 确认数据条款，或走官方覆盖层人工核对。

## 全量快照

`runtime-data/snapshots/` 存全量快照供构建期使用，**不入版本库**
（7 MB+ 且可由脚本重建），已在 `.gitignore` 中排除。

重建命令：

```bash
curl -o runtime-data/snapshots/models_dev.json  https://models.dev/api.json
curl -o runtime-data/snapshots/litellm.json     https://cdn.jsdelivr.net/gh/BerriAI/litellm@main/model_prices_and_context_window.json
curl -o runtime-data/snapshots/openrouter.json  https://openrouter.ai/api/v1/models

cargo run -p runtime-data --bin model-data -- build \
  runtime-data/snapshots/models_dev.json \
  runtime-data/snapshots/litellm.json \
  runtime-data/snapshots/openrouter.json \
  -o runtime-data/out/catalog.json
```

实测结果（2026-09-10）：7619 + 3120 条输入 → **6158 个规范模型**，
944 条检出字段冲突。产出约 14 MB（含完整 evidence 链）。
