"""UI：保存按钮真的落盘并显示结果；启动时恢复；显示已保存密钥。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P, encoding="utf-8").read()

# 1) JobResult 增加 Save
a = a.replace(
    """enum JobResult {
    Connection(Result<runtime_ui::ConnectionReport, BackendError>),
    Discovery(Result<Vec<UiModelEntry>, BackendError>),
}""",
    """enum JobResult {
    Connection(Result<runtime_ui::ConnectionReport, BackendError>),
    Discovery(Result<Vec<UiModelEntry>, BackendError>),
    Save(Result<runtime_ui::SaveReport, BackendError>),
}""",
)

# 2) 字段：保存状态
a = a.replace(
    """    /// 上次刷新的失败原因（**不静默吞掉**，否则用户点了没反应）
    discovery_error: Option<String>,""",
    """    /// 上次刷新的失败原因（**不静默吞掉**，否则用户点了没反应）
    discovery_error: Option<String>,
    /// 上次保存的结果（"已保存" / 失败原因）——保存也必须给出可见反馈
    save_state: SaveState,""",
)
a = a.replace(
    """            discovery_error: None,
            job: None,""",
    """            discovery_error: None,
            save_state: SaveState::Idle,
            job: None,""",
)

# 3) SaveState 类型
a = a.replace(
    """enum JobResult {""",
    """/// 保存动作的界面状态。
#[derive(Debug, Clone, PartialEq, Default)]
enum SaveState {
    #[default]
    Idle,
    Saving,
    Saved {
        /// 是否同时保存了密钥
        credential: bool,
    },
    Failed(String),
}

enum JobResult {""",
)

# 4) 启动恢复
a = a.replace(
    """    /// 首次进入时补齐推荐模型（与切换厂商走同一条路：
    /// 按厂商查随包目录，而不是用硬编码名单）。
    pub fn prime_recommendations(&mut self) {
        self.refresh_recommendations();
    }""",
    """    /// 首次进入时补齐推荐模型（与切换厂商走同一条路：
    /// 按厂商查随包目录，而不是用硬编码名单）。
    pub fn prime_recommendations(&mut self) {
        self.refresh_recommendations();
    }

    /// 启动时恢复上次保存的配置（若宿主实现了持久化）。
    pub fn load_saved(&mut self) {
        if let Some(saved) = self.backend.load_settings() {
            self.state.restore(&saved);
        }
        // 恢复后按恢复出来的厂商取推荐
        self.refresh_recommendations();
        // 已保存的模型要保证在列表里（否则选中项指向一个不存在的行）
        if let Some(model_id) = self.state.selected_model().map(str::to_string) {
            let profile = self.backend.model_info(&model_id);
            self.state
                .add_model(ModelEntry::with_source(model_id, profile, ModelSource::Catalog));
        }
    }

    /// 保存当前配置（供测试与宿主调用）。真正的持久化由后端完成。
    pub fn trigger_save(&mut self) {
        let backend = self.backend.clone();
        let draft = self.state.to_draft();
        let api_key = self.state.api_key().to_string();
        self.save_state = SaveState::Saving;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("umer-ui-save".into())
            .spawn(move || {
                let _ = tx.send(JobResult::Save(backend.save_settings(&draft, &api_key)));
            })
            .ok();
        self.job = Some(Job { rx });
    }

    /// 上次保存的状态（测试用）。
    pub fn save_state(&self) -> &SaveState {
        &self.save_state
    }""",
)

# 5) poll_job 处理 Save
a = a.replace(
    """            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
            }""",
    """            Ok(JobResult::Save(result)) => {
                self.save_state = match result {
                    Ok(report) => {
                        if report.saved_credential {
                            // 密钥已进凭据存储：清出界面状态，只记"有"
                            self.state.mark_key_saved();
                        }
                        SaveState::Saved {
                            credential: report.saved_credential,
                        }
                    }
                    Err(e) => SaveState::Failed(if e.detail.trim().is_empty() {
                        self.text(&e.reason_key)
                    } else {
                        e.detail
                    }),
                };
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
            }""",
)

# 6) 保存按钮：真的调用后端 + 显示结果
old_save = """        ui.horizontal(|ui| {
            let can_save = self.state.is_ready();
            if ui
                .add_enabled(can_save, egui::Button::new(self.text("settings.save")))
                .clicked()
            {
                save = true;
            }"""
new_save = """        ui.horizontal(|ui| {
            let can_save = self.state.is_ready() && self.save_state != SaveState::Saving;
            if ui
                .add_enabled(
                    can_save,
                    egui::Button::new(RichText::new(self.text("settings.save")).strong()),
                )
                .clicked()
            {
                save = true;
            }
            // 保存结果必须可见：成功给确认，失败给原因
            match &self.save_state {
                SaveState::Idle => {}
                SaveState::Saving => {
                    ui.add(egui::Spinner::new().size(13.0));
                    ui.label(RichText::new(self.text("settings.saving")).small());
                }
                SaveState::Saved { credential } => {
                    let text = if *credential {
                        self.text("settings.saved_with_key")
                    } else {
                        self.text("settings.saved")
                    };
                    ui.label(RichText::new(format!("✓ {text}")).small().color(colors.ok));
                }
                SaveState::Failed(reason) => {
                    let message = format!("{} {reason}", self.text("settings.save_failed"));
                    ui.add(
                        egui::Label::new(RichText::new(message).small().color(colors.danger))
                            .wrap(),
                    );
                }
            }"""
assert old_save in a, "save button not found"
a = a.replace(old_save, new_save)

# 7) 保存触发改为真的落盘
a = a.replace(
    """        if save {
            // 无宿主回调时的演示语义：把就绪状态显式呈现
            self.connection = ConnectionTestState::Ok { latency_ms: 0 };
        }""",
    """        if save {
            self.trigger_save();
        }""",
)

# 8) 密钥输入框：已有保存密钥时提示
a = a.replace(
    """        let mut api_key = self.state.api_key().to_string();
        if ui
            .add(
                egui::TextEdit::singleline(&mut api_key)
                    .password(!self.reveal_key)
                    .desired_width((ui.available_width() - 170.0).max(160.0))
                    .hint_text(hint),
            )
            .changed()
        {
            self.state.set_api_key(api_key);
        }""",
    """        let mut api_key = self.state.api_key().to_string();
        let hint = if self.state.has_saved_key() {
            self.text("settings.key.saved_placeholder")
        } else {
            hint
        };
        if ui
            .add(
                egui::TextEdit::singleline(&mut api_key)
                    .password(!self.reveal_key)
                    .desired_width((ui.available_width() - 170.0).max(160.0))
                    .hint_text(hint),
            )
            .changed()
        {
            self.state.set_api_key(api_key);
        }
        if self.state.has_saved_key() && self.state.api_key().trim().is_empty() {
            ui.label(
                RichText::new(self.text("settings.key.saved_badge"))
                    .small()
                    .color(colors.ok),
            );
        }""",
)

# 9) 刷新前的密钥提示改用 needs_key_input
a = a.replace(
    """                if !self.state.keyless() && self.state.api_key().trim().is_empty() {
                    ui.label(
                        RichText::new(self.text("providers.refresh_needs_key"))
                            .small()
                            .color(colors.weak),
                    );
                }""",
    """                if self.state.needs_key_input() {
                    ui.label(
                        RichText::new(self.text("providers.refresh_needs_key"))
                            .small()
                            .color(colors.weak),
                    );
                }""",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(a)
print("app.rs patched")
