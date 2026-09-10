"""列出 models.dev 快照里的 provider 键，用于给 Preset 配 catalog 匹配键。"""
import io
import json

d = json.load(io.open(r"C:\个人文件\API\model-runtime\umber-data\snapshots\models_dev.json", encoding="utf-8"))
keys = sorted(d.keys())
print("providers:", len(keys))
interesting = [
    "deepseek", "zhipu", "zhipuai", "z-ai", "zai", "bigmodel",
    "moonshot", "moonshotai", "kimi",
    "alibaba", "alibabacloud", "dashscope", "qwen",
    "baidu", "qianfan", "ernie",
    "tencent", "hunyuan",
    "minimax", "stepfun", "step", "sensenova", "sensetime",
    "volcengine", "bytedance", "doubao", "ark",
    "siliconflow", "modelscope",
    "openai", "anthropic", "google", "gemini", "xai", "x-ai", "mistral", "groq", "perplexity",
    "openrouter", "together", "togetherai", "fireworks", "nvidia", "cerebras",
]
print("\n-- 匹配到 --")
for k in interesting:
    if k in d:
        n = len(d[k].get("models", {}))
        print(f"  {k:<16} models={n}")
print("\n-- 含关键词的键 --")
for kw in ["zhipu", "moonshot", "alibaba", "baidu", "tencent", "minimax", "step", "sense",
           "volc", "byte", "doubao", "silicon", "modelscope", "deepseek"]:
    hits = [k for k in keys if kw in k]
    if hits:
        print(f"  {kw}: {hits[:6]}")
