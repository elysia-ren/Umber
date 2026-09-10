"""检查产出目录里是否有演示用的模型 ID（用于诊断界面徽标为何缺失）。"""
import io
import json

d = json.load(io.open(r"C:\个人文件\API\model-runtime\umber-data\out\catalog.json", encoding="utf-8"))
entries = d["catalog"]["entries"]
ids = [e["identity"]["canonical_id"] for e in entries]
print("total:", len(ids))
for probe in ["deepseek-chat", "deepseek-reasoner", "gpt-4o-mini", "claude-sonnet-4-5"]:
    print(f"  {probe!r} in catalog:", probe in ids)
print("deepseek*:", [i for i in ids if i.lower().startswith("deepseek")][:12])
print("reasoner*:", [i for i in ids if "reasoner" in i.lower()][:6])
# DeepSeek Reasoner 的显示名来自哪条
hit = [e for e in entries if "reasoner" in e["identity"]["canonical_id"].lower()]
if hit:
    e = hit[0]
    print("sample entry:", e["identity"]["canonical_id"], "| ctx:", e["limits"]["context_window"],
          "| evidence:", len(e["evidence"]))
