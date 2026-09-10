"""从已下载的真实上游数据中裁剪测试夹具（保留真实字段结构）。"""
import json
import os

OUT = r"C:\个人文件\API\model-runtime\umber-data\tests\fixtures"
os.makedirs(OUT, exist_ok=True)

# models.dev：取 2 个 provider，各前 2 个模型
md = json.load(open("md.json", encoding="utf-8"))
out = {}
for pid in ["deepseek", "google"]:
    if pid in md:
        p = md[pid]
        out[pid] = {
            "id": p.get("id"),
            "name": p.get("name"),
            "env": p.get("env"),
            "doc": p.get("doc"),
            "models": dict(list(p["models"].items())[:2]),
        }
json.dump(out, open(os.path.join(OUT, "models_dev_sample.json"), "w", encoding="utf-8"),
          indent=1, ensure_ascii=False)
print("models.dev providers:", list(out.keys()))

# LiteLLM：挑四个已知条目
ll = json.load(open("ll.json", encoding="utf-8"))
pick = [k for k in [
    "deepseek/deepseek-chat",
    "anthropic/claude-3-5-sonnet-20241022",
    "gemini/gemini-2.5-flash",
    "openai/gpt-4o",
] if k in ll]
json.dump({k: ll[k] for k in pick},
          open(os.path.join(OUT, "litellm_sample.json"), "w", encoding="utf-8"),
          indent=1, ensure_ascii=False)
print("litellm keys:", pick)

# OpenRouter：挑带 reasoning.supported_efforts 的
orq = json.load(open("or.json", encoding="utf-8"))
sel = [m for m in orq["data"]
       if m.get("id", "").startswith(("deepseek/", "google/gemini-2.5-flash"))][:3]
json.dump({"data": sel},
          open(os.path.join(OUT, "openrouter_sample.json"), "w", encoding="utf-8"),
          indent=1, ensure_ascii=False)
print("openrouter ids:", [m["id"] for m in sel])
print("reasoning efforts:",
      [(m["id"], m.get("reasoning", {}).get("supported_efforts")) for m in sel])
