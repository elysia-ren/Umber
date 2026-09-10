"""查看重建后 DeepSeek 相关条目的显示名与上下文。"""
import io
import json

d = json.load(io.open(r"C:\个人文件\API\model-runtime\umber-data\out\catalog.json", encoding="utf-8"))
entries = d["catalog"]["entries"]
picked = [e for e in entries if "deepseek" in e["identity"]["canonical_id"].lower()][:8]
print("--- deepseek 条目 ---")
for e in picked:
    print(f"  {e['display_name']:<40} | {e['identity']['canonical_id']:<28} | ctx {e['limits']['context_window']}")

deployments = [z for z in d["catalog"]["deployments"] if z["provider"] == "deepseek"]
print("\ndeepseek deployments:", len(deployments), [z["model_id"] for z in deployments][:8])
for provider in ["zhipuai", "moonshotai", "alibaba", "openai"]:
    ids = [z["model_id"] for z in d["catalog"]["deployments"] if z["provider"] == provider]
    print(f"{provider:<12} deployments={len(ids)}  {ids[:4]}")
