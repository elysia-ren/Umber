"""删除已被 runtime-data 完全吸收的孤儿 crate，并更新文档引用。"""
import io
import os
import shutil

ROOT = r"C:\个人文件\API\model-runtime"

# 1) 删目录
target = os.path.join(ROOT, "catalog-builder")
if os.path.isdir(target):
    shutil.rmtree(target)
    print("removed catalog-builder/")
else:
    print("catalog-builder/ already gone")

# 2) 文档引用
edits = {
    os.path.join(ROOT, "README.md"): [
        (
            "└── catalog-builder/       数据供应链流水线（不随宿主分发）",
            "└── runtime-ffi 之外的数据侧工具见 runtime-data 的 `model-data` CLI",
        ),
    ],
    os.path.join(ROOT, "docs", "HOST_INTEGRATION.md"): [
        (
            "模型元数据由 `catalog-builder` 于构建期生成：",
            "模型元数据由 `runtime-data` 的 `model-data` CLI 于构建期生成：",
        ),
        (
            "catalog-builder source1.json source2.json -o catalog.json",
            "model-data build source1.json source2.json -o catalog.json",
        ),
    ],
    os.path.join(ROOT, "docs", "MODEL_DATA.md"): [
        (
            "| B. Build-time Canonical DB | `catalog-builder` → `runtime-data::pipeline::build` 产出 `Catalog` |",
            "| B. Build-time Canonical DB | `runtime-data::pipeline::build`，由 `model-data build` 驱动 |",
        ),
    ],
}

for path, pairs in edits.items():
    s = io.open(path, encoding="utf-8").read()
    for old, new in pairs:
        if old in s:
            s = s.replace(old, new)
            print("updated:", os.path.basename(path), "|", old[:36])
        else:
            print("MISS   :", os.path.basename(path), "|", old[:36])
    io.open(path, "w", encoding="utf-8", newline="\n").write(s)

# 3) 架构总案里提到 catalog-builder 属历史文档，保留原文不动（它是当时的方案记录）
print("architecture dossier kept as-is (historical record)")
