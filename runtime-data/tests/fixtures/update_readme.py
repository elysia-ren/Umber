"""更新 README 的里程碑行与 crate 列表。"""
import io

P = r"C:\个人文件\API\model-runtime\README.md"
c = io.open(P, encoding="utf-8").read()

old_m10 = "| M10 Runtime UI | **完成**：UISpec 数据契约 + **egui/eframe 参考实现**（主题 token 明暗双套 / 密度 / 缩放 / 系统中文字体 / schema 驱动渲染 / 无头帧测试 / settings-demo 演示，实测 exe 6.7 MB） |"
new_m10 = "| M10 Runtime UI | **完成**：UISpec 数据契约 + egui/eframe 参考实现（侧栏厂商列表 + 内容区、30 个厂商预置分四类、模型数据呈现、证据行、无头测试、截图脚本） |"
if old_m10 in c:
    c = c.replace(old_m10, new_m10)
    print("M10 row updated")
else:
    print("M10 row pattern miss")

old_crate = "├── runtime-ffi/           Stable C ABI（拉取式）+ C/Python 绑定；唯一允许 unsafe 的 crate"
new_crate = (
    "├── runtime-ffi/           Stable C ABI（拉取式）+ C/Python 绑定；唯一允许 unsafe 的 crate\n"
    "├── runtime-data/          Model Data Pipeline & Database（上游适配器 / 规范化 / 冲突解析 / 本地库）"
)
if old_crate in c:
    c = c.replace(old_crate, new_crate)
    print("crate list updated")

old_doc = "- 宿主集成指南：`docs/HOST_INTEGRATION.md`"
if "- 模型数据体系" not in c:
    c = c.replace(old_doc, old_doc + "\n- 模型数据体系：`docs/MODEL_DATA.md`")
    print("docs link added")

io.open(P, "w", encoding="utf-8", newline="\n").write(c)
