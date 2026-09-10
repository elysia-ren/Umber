"""诊断：目录里 canonical_id 是否重复（这会让按 id 索引的查表互相覆盖）。"""
import io
import json
from collections import Counter

d = json.load(io.open(r"C:\个人文件\API\model-runtime\umber-data\out\catalog.json", encoding="utf-8"))
entries = d["catalog"]["entries"]
ids = [e["identity"]["canonical_id"] for e in entries]
counts = Counter(ids)
dups = {k: v for k, v in counts.items() if v > 1}
print("entries:", len(entries), " unique canonical_id:", len(counts), " 重复的 id 个数:", len(dups))
print("重复最多的:", counts.most_common(8))

for probe in ["deepseek-coder", "glm-4.6", "deepseek-chat"]:
    hits = [e for e in entries if e["identity"]["canonical_id"] == probe]
    print(f"\n{probe}: {len(hits)} 条")
    for e in hits[:6]:
        print("   org=", e["identity"].get("organization"),
              "| ctx=", e["limits"].get("context_window"),
              "| evidence=", len(e["evidence"]))
