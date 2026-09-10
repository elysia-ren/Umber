"""收尾：SaveState 公开、i18n 补齐、lib 启动恢复。"""
import io

# 1) SaveState 公开
P_APP = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P_APP, encoding="utf-8").read()
a = a.replace(
    """/// 保存动作的界面状态。
#[derive(Debug, Clone, PartialEq, Default)]
enum SaveState {""",
    """/// 保存动作的界面状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SaveState {""",
)
io.open(P_APP, "w", encoding="utf-8", newline="\n").write(a)
print("app.rs: SaveState public")

# 2) lib 导出 + 启动恢复
P_LIB = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\lib.rs"
l = io.open(P_LIB, encoding="utf-8").read()
l = l.replace("pub use app::SettingsApp;", "pub use app::{SaveState, SettingsApp};")
l = l.replace(
    """            // 开局就把该厂商的目录推荐填上（否则首屏模型列表是空的）
            app.prime_recommendations();""",
    """            // 先把上次保存的配置恢复回来（若有），再按厂商取推荐；
            // 否则用户每次打开都要重新配一遍
            app.load_saved();""",
)
io.open(P_LIB, "w", encoding="utf-8", newline="\n").write(l)
print("lib.rs: wired load_saved")

# 3) i18n
P_STR = r"C:\个人文件\API\model-runtime\runtime-ui\src\strings.rs"
s = io.open(P_STR, encoding="utf-8").read()
if "settings.saving" not in s:
    s = s.replace(
        '    ("settings.saved", "已保存"),',
        '    ("settings.saved", "已保存"),\n'
        '    ("settings.saving", "正在保存…"),\n'
        '    ("settings.saved_with_key", "配置与密钥已保存"),\n'
        '    ("settings.save_failed", "保存失败："),\n'
        '    ("settings.save_unsupported", "当前宿主不支持保存配置"),\n'
        '    ("settings.key.saved_badge", "密钥已保存到系统凭据存储"),\n'
        '    ("settings.key.saved_placeholder", "已保存（留空则沿用）"),',
    )
    s = s.replace(
        '    ("settings.saved", "Saved"),',
        '    ("settings.saved", "Saved"),\n'
        '    ("settings.saving", "Saving…"),\n'
        '    ("settings.saved_with_key", "Configuration and key saved"),\n'
        '    ("settings.save_failed", "Save failed: "),\n'
        '    ("settings.save_unsupported", "This host does not support saving settings"),\n'
        '    ("settings.key.saved_badge", "Key stored in the system credential store"),\n'
        '    ("settings.key.saved_placeholder", "Saved (leave empty to keep)"),',
    )
    io.open(P_STR, "w", encoding="utf-8", newline="\n").write(s)
    print("strings patched")
else:
    print("strings already present")

# 4) 字符串覆盖测试补新 key
s = io.open(P_STR, encoding="utf-8").read()
if '"settings.saving"' not in s.split("APP_KEYS")[1]:
    s = s.replace(
        '            "settings.saved",\n            "models.none",',
        '            "settings.saved",\n            "settings.saving",\n'
        '            "settings.saved_with_key",\n            "settings.save_failed",\n'
        '            "settings.save_unsupported",\n            "settings.key.saved_badge",\n'
        '            "settings.key.saved_placeholder",\n            "models.none",',
    )
    io.open(P_STR, "w", encoding="utf-8", newline="\n").write(s)
    print("string coverage test updated")
