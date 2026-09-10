"""把 README 里指向仓库外部的架构文档链接改为仓库内路径。"""
import io

P = r"C:\个人文件\API\model-runtime\README.md"
s = io.open(P, encoding="utf-8").read()

pairs = [
    (
        "- 架构总案：`../Universal Embedded Model Runtime：V0 架构总案（修订版）.md`",
        "- 架构总案：[`docs/architecture/V0-架构总案.md`](docs/architecture/V0-架构总案.md)（初版存于 [`docs/architecture/V0-架构总案-初版.md`](docs/architecture/V0-架构总案-初版.md)）",
    ),
    (
        "- 开发计划：`../Universal Embedded Model Runtime：开发计划.md`",
        "- 开发计划：[`docs/architecture/开发计划.md`](docs/architecture/开发计划.md)",
    ),
]

for old, new in pairs:
    if old in s:
        s = s.replace(old, new)
        print("updated:", old[:30])
    elif new.split("](")[0] in s:
        print("already updated:", old[:30])
    else:
        print("pattern miss:", old[:40])

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
