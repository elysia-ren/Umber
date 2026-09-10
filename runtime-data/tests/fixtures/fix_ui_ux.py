"""UX 收尾：明确"未选中模型"的提示文案 + 进入界面时默认选中首个模型。"""
import io

# ---- 1) 新增 i18n key ----
P_STR = r"C:\个人文件\API\model-runtime\runtime-ui\src\strings.rs"
s = io.open(P_STR, encoding="utf-8").read()
if "model.select_hint" not in s:
    s = s.replace(
        '    ("model.data.absent", "目录中没有该模型的数据（仍可正常使用）"),',
        '    ("model.data.absent", "目录中没有该模型的数据（仍可正常使用）"),\n'
        '    ("model.select_hint", "选择上方任一模型，查看能力、价格与上下文"),',
    )
    s = s.replace(
        '    ("model.data.absent", "No catalog data for this model (still usable)"),',
        '    ("model.data.absent", "No catalog data for this model (still usable)"),\n'
        '    ("model.select_hint", "Pick a model above to see capabilities, pricing and context"),',
    )
    io.open(P_STR, "w", encoding="utf-8", newline="\n").write(s)
    print("strings: added model.select_hint")
else:
    print("strings: already present")

# ---- 2) app.rs：未选中模型时用专用提示；进入时默认选中首个模型 ----
P_APP = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P_APP, encoding="utf-8").read()

old_hint = """        let Some(model_id) = self.state.selected_model().map(str::to_string) else {
            ui.label(RichText::new(self.text("wizard.models.empty")).color(colors.weak));
            return;
        };"""
new_hint = """        let Some(model_id) = self.state.selected_model().map(str::to_string) else {
            // 列表里有模型但尚未选中：给出明确指引（而不是重复"还没获取模型")
            let key = if self.state.models().is_empty() {
                "wizard.models.empty"
            } else {
                "model.select_hint"
            };
            ui.add(egui::Label::new(RichText::new(self.text(key)).color(colors.weak)).wrap());
            return;
        };"""
assert old_hint in a, "empty-state block not found"
a = a.replace(old_hint, new_hint)

# 默认选中首个模型：右侧面板一进来就有内容，而不是空白
old_enrich_tail = """        for model_id in missing {
            self.queried.insert(model_id.clone());
            if let Some(profile) = self.backend.model_info(&model_id) {
                self.state.add_model(ModelEntry::new(model_id, Some(profile)));
            }
        }
    }"""
new_enrich_tail = """        for model_id in missing {
            self.queried.insert(model_id.clone());
            if let Some(profile) = self.backend.model_info(&model_id) {
                self.state.add_model(ModelEntry::new(model_id, Some(profile)));
            }
        }
        // 尚未选模型时默认选中第一个：避免右侧一片空白，也少一次点击
        if self.state.selected_model().is_none() {
            if let Some(first) = self.state.models().first() {
                let first = first.model_id.clone();
                self.state.select_model(first);
            }
        }
    }"""
assert old_enrich_tail in a, "enrich tail not found"
a = a.replace(old_enrich_tail, new_enrich_tail)

io.open(P_APP, "w", encoding="utf-8", newline="\n").write(a)
print("app.rs: patched")
