"""修正 UI 宽度问题：长文本必须换行，否则会把内容区撑宽导致右侧被裁切。"""
import io
import re

P = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
s = io.open(P, encoding="utf-8").read()

# 1) 请求预览：长 URL 必须换行
old = """        if let Some(preview) = &self.request_preview {
            ui.label(
                RichText::new(format!("{} {}", self.text("providers.request_to"), preview))
                    .small()
                    .color(colors.weak),
            );
        }"""
new = """        if let Some(preview) = &self.request_preview {
            // 必须 wrap：长 URL 不换行会把内容区撑宽，右侧控件被裁掉
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{} {}", self.text("providers.request_to"), preview))
                        .small()
                        .color(colors.weak),
                )
                .wrap(),
            );
        }"""
assert old in s, "preview label not found"
s = s.replace(old, new)

# 2) 空态与说明类长文本统一 wrap
s = s.replace(
    'ui.label(RichText::new(self.text("wizard.models.empty")).color(colors.weak));',
    'ui.add(egui::Label::new(RichText::new(self.text("wizard.models.empty")).color(colors.weak)).wrap());',
)
s = s.replace(
    'ui.label(RichText::new(self.text("model.data.absent"))\n                        .small()\n                        .color(colors.weak),',
    'ui.add(egui::Label::new(RichText::new(self.text("model.data.absent")).small().color(colors.weak)).wrap());',
)

# 3) API Key 行：给两个按钮留足宽度
s = s.replace(
    '.desired_width(ui.available_width() - 96.0)',
    '.desired_width((ui.available_width() - 170.0).max(160.0))',
)
# 4) 协议下拉不要超出
s = s.replace('.selected_text(current_label)\n                .width(200.0)', '.selected_text(current_label)\n                .width(180.0)')

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("app.rs patched")
