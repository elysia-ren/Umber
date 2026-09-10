//! 设置窗口的立即模式 UI。
//!
//! 渲染是**从 schema 生成**的：`render_field` 只认 `FieldKind`，
//! schema 增删字段时布局代码零改动——这正是 UISpec 契约的证明方式。
//! 动作全部走 `SettingsBackend` 并在工作线程执行。

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

use egui::{Context, TextEdit, Ui};
use runtime_ui::{
    BackendError, CapabilityStatus, ConnectionTestState, DiscoverySession, DiscoveryState,
    FieldKind, SettingsBackend, SettingsDraft, SettingsPage, Strings, UiModelEntry, UiModelInfo,
};

use crate::theme::{semantic, Density, ThemeMode, UiTheme};

/// 单个后台动作的结果。
enum JobResult {
    Connection(Result<runtime_ui::ConnectionReport, BackendError>),
    Discovery(Result<Vec<UiModelEntry>, BackendError>),
}

struct Job {
    rx: Receiver<JobResult>,
}

pub struct SettingsApp {
    page: SettingsPage,
    strings: Strings,
    backend: Arc<dyn SettingsBackend>,

    pub theme: UiTheme,
    draft: SettingsDraft,
    connection: ConnectionTestState,
    discovery: DiscoverySession,
    manual_model: String,
    models: Vec<UiModelInfo>,
    selected_model: Option<String>,
    job: Option<Job>,
}

impl SettingsApp {
    pub fn new(page: SettingsPage, strings: Strings, backend: Arc<dyn SettingsBackend>) -> Self {
        // 以 schema 默认值播种草稿：validate / 渲染的起点与 Core 一致
        let mut draft = SettingsDraft::default();
        for section in &page.sections {
            for field in &section.fields {
                if let Some(default) = &field.default {
                    draft
                        .values
                        .entry(field.id.clone())
                        .or_insert_with(|| default.clone());
                }
            }
        }
        Self {
            page,
            strings,
            backend,
            theme: UiTheme::default(),
            draft,
            connection: ConnectionTestState::Idle,
            discovery: DiscoverySession::new(),
            manual_model: String::new(),
            models: Vec::new(),
            selected_model: None,
            job: None,
        }
    }

    pub fn apply_theme(&mut self, ctx: &Context, mode: ThemeMode, density: Density, scale: f32) {
        self.theme.mode = mode;
        self.theme.density = density;
        self.theme.scale = scale;
        crate::theme::apply(ctx, mode, density, scale);
    }

    pub fn install_system_fonts(&self, ctx: &Context) {
        let _ = crate::fonts::install_cjk_fallback(ctx);
    }

    /// 渲染一帧。公开以支持无头测试（不开窗口即可验证渲染路径）。
    pub fn ui(&mut self, ctx: &Context) {
        self.poll_job();
        egui::CentralPanel::default().show(ctx, |ui| {
            self.header(ui);
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                // 按索引迭代：render_field 需要 &mut self，不能持有 sections 借用
                for si in 0..self.page.sections.len() {
                    ui.add_space(6.0);
                    ui.heading(self.text(&self.page.sections[si].title_key));
                    for fi in 0..self.page.sections[si].fields.len() {
                        let field = self.page.sections[si].fields[fi].clone();
                        self.render_field(ui, &field);
                    }
                }
                ui.add_space(10.0);
                self.connection_row(ui);
                ui.separator();
                self.discovery_section(ui);
                self.models_section(ui);
            });
        });
    }

    fn text(&self, key: &str) -> String {
        let value = self.strings.get(key);
        if value.is_empty() {
            key.to_string() // 缺 key 时显示键名，绝不空白（调试可见）
        } else {
            value.to_string()
        }
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let title = self.text(&self.page.title_key);
            ui.add(egui::Label::new(egui::RichText::new(title).heading()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode_label = if self.theme.mode == ThemeMode::Dark {
                    "☀"
                } else {
                    "☾"
                };
                if ui.button(mode_label).clicked() {
                    let next = if self.theme.mode == ThemeMode::Dark {
                        ThemeMode::Light
                    } else {
                        ThemeMode::Dark
                    };
                    let ctx = ui.ctx().clone();
                    let (mode, density, scale) = (next, self.theme.density, self.theme.scale);
                    crate::theme::apply(&ctx, mode, density, scale);
                    self.theme.mode = next;
                }
            });
        });
    }

    fn field_value(&self, field_id: &str) -> String {
        self.draft.values.get(field_id).cloned().unwrap_or_default()
    }

    fn render_field(&mut self, ui: &mut Ui, field: &runtime_ui::FieldSpec) {
        ui.add_space(2.0);
        ui.label(self.text(&field.label_key));

        match &field.kind {
            FieldKind::Textarea => {
                let mut value = self.field_value(&field.id);
                let response = TextEdit::multiline(&mut value)
                    .desired_rows(3)
                    .id_salt(("umer-field", &field.id))
                    .show(ui)
                    .response;
                if response.changed() {
                    self.draft.values.insert(field.id.clone(), value);
                }
            }
            FieldKind::Secret => {
                let mut value = self.field_value(&field.id);
                let response = TextEdit::singleline(&mut value)
                    .password(true)
                    .id_salt(("umer-field", &field.id))
                    .show(ui)
                    .response;
                if response.changed() {
                    self.draft.values.insert(field.id.clone(), value);
                }
            }
            FieldKind::Text => {
                let mut value = self.field_value(&field.id);
                let response = TextEdit::singleline(&mut value)
                    .id_salt(("umer-field", &field.id))
                    .show(ui)
                    .response;
                if response.changed() {
                    self.draft.values.insert(field.id.clone(), value);
                }
            }
            FieldKind::Toggle => {
                let mut on = self.field_value(&field.id) == "true";
                if ui.checkbox(&mut on, "").changed() {
                    self.draft
                        .values
                        .insert(field.id.clone(), if on { "true" } else { "false" }.into());
                }
            }
            FieldKind::Select { options } => {
                let current = self.field_value(&field.id);
                let current_label = options
                    .iter()
                    .find(|o| o.value == current)
                    .map(|o| self.text(&o.label_key))
                    .unwrap_or_else(|| current.clone());
                let mut picked: Option<String> = None;
                egui::ComboBox::from_id_salt(("umer-field", &field.id))
                    .selected_text(current_label)
                    .width(ui.available_width().min(320.0))
                    .show_ui(ui, |ui| {
                        for option in options {
                            let label = self.text(&option.label_key);
                            if ui
                                .selectable_label(option.value == current, label)
                                .clicked()
                            {
                                picked = Some(option.value.clone());
                            }
                        }
                    });
                if let Some(value) = picked {
                    self.draft.values.insert(field.id.clone(), value);
                }
            }
        }

        if let Some(help) = &field.help_key {
            let colors = semantic(self.theme.mode);
            ui.add(egui::Label::new(
                egui::RichText::new(self.text(help))
                    .small()
                    .color(colors.weak),
            ));
        }
        // 校验问题就地显示（Core 侧 validate 的结果，§38.1）
        for issue in self.page.validate(&self.draft) {
            if issue.field_id == field.id {
                let colors = semantic(self.theme.mode);
                ui.label(
                    egui::RichText::new(self.text(&issue.issue_key))
                        .small()
                        .color(colors.danger),
                );
            }
        }
    }

    fn connection_row(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let colors = semantic(self.theme.mode);
            let testing = self.connection == ConnectionTestState::Testing;
            if ui
                .add_enabled(
                    !testing,
                    egui::Button::new(self.text("settings.provider.probe")),
                )
                .clicked()
            {
                self.start_connection_test();
            }
            match &self.connection {
                ConnectionTestState::Idle => {}
                ConnectionTestState::Testing => {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(self.text("connection.testing"));
                }
                ConnectionTestState::Ok { latency_ms } => {
                    ui.label(
                        egui::RichText::new(format!(
                            "✓ {} · {} {}ms",
                            self.text("connection.ok"),
                            self.text("connection.latency"),
                            latency_ms
                        ))
                        .color(colors.ok),
                    );
                }
                ConnectionTestState::Failed { reason_key } => {
                    ui.label(egui::RichText::new(self.text(reason_key)).color(colors.danger));
                }
            }
        });
    }

    fn start_connection_test(&mut self) {
        self.connection = ConnectionTestState::Testing;
        let backend = self.backend.clone();
        let draft = self.draft.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("umer-ui-action".into())
            .spawn(move || {
                let _ = tx.send(JobResult::Connection(backend.test_connection(&draft)));
            })
            .ok();
        self.job = Some(Job { rx });
    }

    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        match job.rx.try_recv() {
            Ok(JobResult::Connection(result)) => {
                self.connection = match result {
                    Ok(report) => ConnectionTestState::Ok {
                        latency_ms: report.latency_ms,
                    },
                    Err(e) => ConnectionTestState::Failed {
                        reason_key: e.reason_key,
                    },
                };
                self.job = None;
            }
            Ok(JobResult::Discovery(result)) => {
                let _ = match result {
                    Ok(models) => self.discovery.succeed(models),
                    Err(e) => self.discovery.fail(&e.reason_key),
                };
                self.refresh_models();
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
            }
        }
    }

    fn discovery_section(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.heading(self.text("discovery.title"));
        ui.horizontal(|ui| {
            let busy = matches!(self.discovery.state(), DiscoveryState::Discovering);
            if ui
                .add_enabled(!busy, egui::Button::new(self.text("discovery.auto")))
                .clicked()
            {
                let _ = self.discovery.begin();
                let backend = self.backend.clone();
                let draft = self.draft.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("umer-ui-discovery".into())
                    .spawn(move || {
                        let _ = tx.send(JobResult::Discovery(backend.discover(&draft)));
                    })
                    .ok();
                self.job = Some(Job { rx });
            }
            match self.discovery.state() {
                DiscoveryState::Idle => {}
                DiscoveryState::Discovering => {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(self.text("discovery.in_progress"));
                }
                DiscoveryState::Failed { reason_key } => {
                    let colors = semantic(self.theme.mode);
                    ui.label(egui::RichText::new(self.text(reason_key)).color(colors.danger));
                }
                DiscoveryState::Done { models } => {
                    ui.label(format!("{} 项", models.len()));
                }
            }
        });
        // 失败/空闲都可手动添加（总案 §36：发现失败不阻断）
        ui.horizontal(|ui| {
            ui.label(self.text("discovery.manual.add"));
            let response = ui.text_edit_singleline(&mut self.manual_model);
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || ui.button(self.text("discovery.add")).clicked())
                && self.discovery.add_manual_model(&self.manual_model).is_ok()
            {
                self.refresh_models();
                self.manual_model.clear();
            }
        });
    }

    /// 把 Discovery 结果 + 后端已知知识合成为模型展示列表。
    fn refresh_models(&mut self) {
        self.models = match self.discovery.state() {
            DiscoveryState::Done { models } => models
                .iter()
                .map(|entry: &UiModelEntry| {
                    self.backend
                        .model_info(&entry.model_id)
                        .unwrap_or_else(|| UiModelInfo::all_unknown(entry.model_id.clone()))
                })
                .collect(),
            _ => Vec::new(),
        };
        self.selected_model = self.models.first().map(|m| m.model_id.clone());
    }

    fn models_section(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.heading(self.text("model.capabilities"));
        if self.models.is_empty() {
            ui.weak(self.text("models.none"));
            return;
        }
        for model in &self.models {
            let label = model
                .display_name
                .clone()
                .unwrap_or_else(|| model.model_id.clone());
            let selected = self.selected_model.as_deref() == Some(model.model_id.as_str());
            if ui.selectable_label(selected, &label).clicked() {
                self.selected_model = Some(model.model_id.clone());
            }
        }
        // 能力矩阵：Unknown 如实显示，绝不美化（§15 §16）
        let Some(selected) = self.selected_model.clone() else {
            return;
        };
        let Some(info) = self.models.iter().find(|m| m.model_id == selected) else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            let colors = semantic(self.theme.mode);
            if info.capabilities.is_empty() {
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {}",
                        self.text("model.capabilities"),
                        self.text("model.capability.unknown")
                    ))
                    .color(colors.weak),
                );
                return;
            }
            for (kind, status) in &info.capabilities {
                let (key, color) = match status {
                    CapabilityStatus::Supported => ("model.capability.supported", colors.ok),
                    CapabilityStatus::Unsupported => ("model.capability.unsupported", colors.weak),
                    CapabilityStatus::Partial => ("model.capability.partial", colors.accent),
                    CapabilityStatus::Unknown => ("model.capability.unknown", colors.weak),
                };
                ui.label(egui::RichText::new(format!("{kind:?}: {}", self.text(key))).color(color));
            }
        });
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}
