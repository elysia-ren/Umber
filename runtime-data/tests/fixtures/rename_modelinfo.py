"""把 ModelInfo 重命名为 ModelProfile（规格 X.2 的正式名称），model.rs 除外。"""
import io
import os

ROOT = r"C:\个人文件\API\model-runtime"
SKIP_FILE = os.path.join(ROOT, "runtime-model", "src", "model.rs")

patched = []
for dirpath, dirnames, filenames in os.walk(ROOT):
    if "target" in dirpath.split(os.sep) or ".git" in dirpath.split(os.sep):
        continue
    for name in filenames:
        if not name.endswith(".rs"):
            continue
        path = os.path.join(dirpath, name)
        if path == SKIP_FILE:
            continue
        with io.open(path, encoding="utf-8") as f:
            text = f.read()
        if "ModelInfo" not in text:
            continue
        # 只替换独立标识符，避免误伤
        out = []
        i = 0
        while True:
            j = text.find("ModelInfo", i)
            if j < 0:
                out.append(text[i:])
                break
            before = text[j - 1] if j > 0 else ""
            after = text[j + len("ModelInfo")] if j + len("ModelInfo") < len(text) else ""
            if (before.isalnum() or before == "_") or (after.isalnum() or after == "_"):
                out.append(text[i:j + len("ModelInfo")])
            else:
                out.append(text[i:j])
                out.append("ModelProfile")
            i = j + len("ModelInfo")
        new_text = "".join(out)
        if new_text != text:
            with io.open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write(new_text)
            patched.append(os.path.relpath(path, ROOT))

print("\n".join(patched))
print("total:", len(patched))
