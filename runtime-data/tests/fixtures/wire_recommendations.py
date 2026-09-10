"""app.rs + demo 后端：切厂商时按厂商查目录推荐；刷新失败要显示原因。"""
import io

# ---------- app.rs ----------
P = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P, encoding="utf-8").read()

# 1) 切厂商后请求推荐（替代之前的硬编码）
old = """        if let Some(id) = pick {
            self.state.select_provider(id);
            self.connection = ConnectionTestState::Idle;
            self.request_preview = None;
        }
    }"""
new = """        if let Some(id) = pick {
            self.state.select_provider(id);
            self.connection = ConnectionTestState::Idle;
            self.request_preview = None;
            self.discovery_error = None;
            self.refresh_recommendations();
        }
    }

    /// 按当前厂商向 Core 要推荐模型（查随包目录，不是硬编码名字）。
    ///
    /// 目录里没有该厂商时列表会是空的——界面据此提示"刷新模型列表"或手填，
    /// 而不是显示过时型号。
    fn refresh_recommendations(&mut self) {
        let ids: Vec<String> = self
            .state
            .preset()
            .catalog_provider_ids
            .iter()
            .map(|s| s.to_string())
            .collect();
        if ids.is_empty() {
            return; // 本地部署 / 自定义：没有目录来源
        }
        let entries: Vec<ModelEntry> = self
            .backend
            .recommend_models(&ids, 12)
            .into_iter()
            .map(|e| {
                let profile = self.backend.model_info(&e.model_id);
                ModelEntry::with_source(e.model_id, profile, ModelSource::Catalog)
            })
            .collect();
        self.state.apply_recommendations(entries);
    }"""
assert old in a, "provider pick block not found"
a = a.replace(old, new)

# 2) 结构体加 discovery_error 字段
a = a.replace(
    "    /// 已向后端查询过知识的模型 ID（避免每帧重复查询）\n    queried: std::collections::HashSet<String>,",
    "    /// 已向后端查询过知识的模型 ID（避免每帧重复查询）\n    queried: std::collections::HashSet<String>,\n    /// 上次刷新的失败原因（**不再静默吞掉**）\n    discovery_error: Option<String>,",
)
a = a.replace(
    "            queried: std::collections::HashSet::new(),\n            job: None,",
    "            queried: std::collections::HashSet::new(),\n            discovery_error: None,\n            job: None,",
)

# 3) 发现失败时记录原因而不是忽略
old_fail = """                    Err(_) => {
                        // 发现失败不阻断：用户可以手动添加（§36）
                    }"""
new_fail = """                    Err(e) => {
                        // 不阻断（可手填，§36），但**必须让用户看到为什么**
                        self.discovery_error = Some(if e.detail.is_empty() {
                            e.reason_key
                        } else {
                            e.detail
                        });
                    }"""
if old_fail in a:
    a = a.replace(old_fail, new_fail)
    print("discovery error surfaced")
else:
    print("discovery error pattern miss")

# 4) 刷新按钮：无密钥且非免密时先提示需要 Key
old_refresh = """                let busy = self.job.is_some();
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(RichText::new(self.text("providers.refresh_models")).small()),
                    )
                    .clicked()
                {
                    self.start_discovery();
                }"""
new_refresh = """                let busy = self.job.is_some();
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(RichText::new(self.text("providers.refresh_models")).small()),
                    )
                    .clicked()
                {
                    self.start_discovery();
                }
                // 无密钥时明说刷新会失败（而不是点了没反应）
                if !self.state.keyless() && self.state.api_key().trim().is_empty() {
                    ui.label(
                        RichText::new(self.text("providers.refresh_needs_key"))
                            .small()
                            .color(colors.weak),
                    );
                }"""
assert old_refresh in a, "refresh button not found"
a = a.replace(old_refresh, new_refresh)

# 5) 发现失败信息显示在模型列表上方
old_hint = """        ui.add_space(4.0);
        self.model_rows(ui);"""
new_hint = """        if let Some(error) = &self.discovery_error {
            let message = format!("{} {}", self.text("providers.refresh_failed"), error);
            ui.add(
                egui::Label::new(RichText::new(message).small().color(colors.danger)).wrap(),
            );
        }
        ui.add_space(4.0);
        self.model_rows(ui);"""
assert old_hint in a, "model rows call not found"
a = a.replace(old_hint, new_hint)

# 6) 起始也要拉一次推荐（否则首屏列表为空）
old_new = """    pub fn state_mut(&mut self) -> &mut SettingsState {"""
new_new = """    /// 首次进入时补齐推荐模型（与切换厂商走同一条路）。
    pub fn prime_recommendations(&mut self) {
        self.refresh_recommendations();
    }

    pub fn state_mut(&mut self) -> &mut SettingsState {"""
assert old_new in a, "state_mut not found"
a = a.replace(old_new, new_new)

# 7) 导入 ModelSource / ModelEntry
a = a.replace(
    "use runtime_ui::{\n    BackendError, CapabilityStatus, ConnectionTestState, ContextWindowEvidence, ModelEntry,\n    ProtocolKind, ProviderPreset, SettingsBackend, SettingsState, Strings, UiModelEntry,\n    UiModelInfo,\n};",
    "use runtime_ui::{\n    BackendError, CapabilityStatus, ConnectionTestState, ContextWindowEvidence, ModelEntry,\n    ModelSource, ProtocolKind, ProviderPreset, SettingsBackend, SettingsState, Strings,\n    UiModelEntry, UiModelInfo,\n};",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(a)
print("app.rs patched")
