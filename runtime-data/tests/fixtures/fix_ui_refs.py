"""一次性修正：settings_state 的 set_probed_context 与 strings 测试的过时引用。"""
import io

# 1) settings_state.rs: set_probed_context 误用了 ?
p1 = r"C:\个人文件\API\model-runtime\runtime-ui\src\settings_state.rs"
s = io.open(p1, encoding="utf-8").read()
old = """    pub fn set_probed_context(&mut self, value: Option<u64>) {
        self.probed_context = Some(value?).or(None);
        if let Some(v) = value {
            self.probed_context = Some(v);
        }
    }"""
new = """    pub fn set_probed_context(&mut self, value: Option<u64>) {
        self.probed_context = value;
    }"""
if old in s:
    s = s.replace(old, new)
    io.open(p1, "w", encoding="utf-8", newline="\n").write(s)
    print("settings_state: patched")
else:
    print("settings_state: pattern not found (maybe already fixed)")

# 2) strings.rs: 测试里引用了已删除的 wizard 模块与 note_key
p2 = r"C:\个人文件\API\model-runtime\runtime-ui\src\strings.rs"
s = io.open(p2, encoding="utf-8").read()
s = s.replace("use crate::wizard::Step;", "use crate::preset::ProviderCategory;")
old_loop = """        for step in [Step::Provider, Step::Credentials, Step::Model] {
            keys.push(step.title_key());
        }
"""
if old_loop in s:
    s = s.replace(old_loop, "")
s = s.replace("keys.push(preset.note_key);", "keys.push(preset.subtitle_key());")
if "ProviderCategory" in s and "Step::" not in s:
    io.open(p2, "w", encoding="utf-8", newline="\n").write(s)
    print("strings: patched")
else:
    print("strings: still has Step:: references")
