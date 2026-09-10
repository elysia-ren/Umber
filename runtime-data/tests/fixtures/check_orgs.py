"""诊断：目录里 organization/deepseek 的条目有多少，推荐为什么只出一个。"""
import io
import json

d = json.load(io.open(r"C:\个人文件\API\model-runtime\runtime-data\out\catalog.json", encoding="utf-8"))
entries = d["catalog"]["entries"]

by_org = {}
for e in entries:
    org = (e["identity"].get("organization") or "?")
    by_org.setdefault(org, []).append(e["identity"]["canonical_id"])

for org in ["deepseek", "zhipuai", "alibaba", "moonshotai", "openai", "anthropic"]:
    ids = by_org.get(org, [])
    print(f"{org:<12} {len(ids):>4}  {ids[:6]}")

print()
print("含 deepseek 的 canonical_id:", [i for i in by_org.get("?", []) if "deepseek" in i.lower()][:8])
print("organization 为空的条目数:", len(by_org.get("?", [])))
print("organization 取值前 20:", sorted(by_org.keys())[:20])
