"""收尾修正：能力缺失措辞、上下文徽标记法、窗口高度。"""
import io

# ---- 1) i18n：区分"目录无该模型"与"目录未记录该能力字段" ----
P_STR = r"C:\个人文件\API\model-runtime\runtime-ui\src\strings.rs"
s = io.open(P_STR, encoding="utf-8").read()
if "model.capability.unlisted" not in s:
    s = s.replace(
        '    ("model.select_hint", "选择上方任一模型，查看能力、价格与上下文"),',
        '    ("model.select_hint", "选择上方任一模型，查看能力、价格与上下文"),\n'
        '    ("model.capability.unlisted", "目录未记录该能力字段"),',
    )
    s = s.replace(
        '    ("model.select_hint", "Pick a model above to see capabilities, pricing and context"),',
        '    ("model.select_hint", "Pick a model above to see capabilities, pricing and context"),\n'
        '    ("model.capability.unlisted", "The catalog does not record this capability"),',
    )
    io.open(P_STR, "w", encoding="utf-8", newline="\n").write(s)
    print("strings: added model.capability.unlisted")

# ---- 2) app.rs：能力为空时按"是否有目录条目"区分措辞 ----
P_APP = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P_APP, encoding="utf-8").read()
old = """            if capabilities.is_empty() {
                // 目录里没有这个模型：如实说明，而不是留空
                ui.add(egui::Label::new(RichText::new(self.text("model.data.absent")).small().color(colors.weak)).wrap());
            } else {"""
new = """            if capabilities.is_empty() {
                // 区分两种"没有"：目录里根本没有这个模型 / 目录有它但没记这个字段。
                // 说清区别才是诚实——有价格却显示"无数据"是误导。
                let key = if self.state.selected_profile().is_some() {
                    "model.capability.unlisted"
                } else {
                    "model.data.absent"
                };
                ui.add(
                    egui::Label::new(RichText::new(self.text(key)).small().color(colors.weak))
                        .wrap(),
                );
            } else {"""
assert old in a, "capability empty branch not found"
a = a.replace(old, new)

# ---- 3) 上下文徽标：按 1024 进制显示 64K / 1M，符合从业者阅读习惯 ----
old_short = """fn short_tokens(value: u64) -> String {
    if value >= 1_000_000 && value % 1_000_000 == 0 {
        format!("{}M", value / 1_000_000)
    } else if value >= 1_000 && value % 1_000 == 0 {
        format!("{}K", value / 1_000)
    } else {
        value.to_string()
    }
}"""
new_short = """fn short_tokens(value: u64) -> String {
    // 上下文窗口从业者按 1024 进制读（65536 = 64K、1048576 = 1M）
    const K: u64 = 1024;
    const M: u64 = 1024 * 1024;
    if value >= M && value % M == 0 {
        format!("{}M", value / M)
    } else if value >= K * 8 && value % K == 0 {
        format!("{}K", value / K)
    } else {
        value.to_string()
    }
}"""
assert old_short in a, "short_tokens not found"
a = a.replace(old_short, new_short)

# 测试同步
a = a.replace(
    '''        assert_eq!(short_tokens(1_048_576), "1048576");
        assert_eq!(short_tokens(1_000_000), "1M");
        assert_eq!(short_tokens(128_000), "128K");
        assert_eq!(short_tokens(8_192), "8192");''',
    '''        // 按 1024 进制读：65536=64K、1048576=1M
        assert_eq!(short_tokens(65_536), "64K");
        assert_eq!(short_tokens(1_048_576), "1M");
        // 非 1024 整数倍的原样显示（128000 确实是 128000）
        assert_eq!(short_tokens(128_000), "128000");
        assert_eq!(short_tokens(8_192), "8K");
        assert_eq!(short_tokens(512), "512");''',
)
io.open(P_APP, "w", encoding="utf-8", newline="\n").write(a)
print("app.rs: patched")

# ---- 4) 窗口高度略增，让思考强度那行也露出来 ----
P_LIB = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\lib.rs"
l = io.open(P_LIB, encoding="utf-8").read()
l = l.replace(".with_inner_size([940.0, 720.0])", ".with_inner_size([940.0, 800.0])")
io.open(P_LIB, "w", encoding="utf-8", newline="\n").write(l)
print("lib.rs: window height 800")
