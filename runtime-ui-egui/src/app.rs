//! 设置界面：左侧厂商列表 + 右侧配置（对齐现实产品的用法）。
//!
//! ```text
//! ┌──────────────┬────────────────────────────────────┐
//! │ 搜索厂商      │ 厂商名 [当前]        连接方式 ▾    │
//! │ ───────────  │ API Key  [••••]  [获取]            │
//! │ 官方          │ Base URL [https://…]               │
//! │  OpenAI      │ 请求将发送到 https://…/chat/completions │
//! │  DeepSeek ●  │ 模型列表                            │
//! │ 国内          │  [GLM-5.3  1M  工具 视觉]  🗑       │
//! │  智谱 GLM    │  [+ 添加模型]  [刷新模型列表]        │
//! │ …            │ 上下文窗口 [1024000]                │
//! │              │ 自动探测 256,000 · 目录 64,000 · 生效 1,024,000 │
//! └──────────────┴────────────────────────────────────┘
//!            [测试连接]              [保存配置]
//! ```
//!
//! 渲染纪律：
//! - 逻辑全在 `runtime_ui::SettingsState`，本文件只画界面、转交事件
//! - 网络动作在工作线程执行，渲染线程永不阻塞
//! - 未知就说未知（能力/档位），不美化、不猜（§15 §16）

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

use egui::{Context, RichText, Ui};
use runtime_ui::{
    BackendError, CapabilityStatus, ConnectionTestState, ContextWindowEvidence, ModelEntry,
    ModelSource, ProtocolKind, ProviderPreset, SettingsBackend, SettingsState, Strings,
    UiModelEntry, UiModelInfo,
};

use crate::theme::{semantic, Density, ThemeMode, UiTheme};

enum JobResult {
    Connection(Result<runtime_ui::ConnectionReport, BackendError>),
    Discovery(Result<Vec<UiModelEntry>, BackendError>),
}

struct Job {
    rx: Receiver<JobResult>,
}

pub struct SettingsApp {
    strings: Strings,
    backend: Arc<dyn SettingsBackend>,
    pub theme: UiTheme,
    state: SettingsState,
    connection: ConnectionTestState,
    /// 请求 URL 预览（由后端计算，保证与 Adapter 实际拼接一致）
    request_preview: Option<String>,
    manual_model: String,
    /// 密钥是否明文显示
    reveal_key: bool,
    /// 已向后端查询过知识的模型 ID（避免每帧重复查询）
    queried: std::collections::HashSet<String>,
    /// 上次刷新的失败原因（**不静默吞掉**，否则用户点了没反应）
    discovery_error: Option<String>,
    job: Option<Job>,
}

impl SettingsApp {
    pub fn new(
        _page: runtime_ui::SettingsPage,
        strings: Strings,
        backend: Arc<dyn SettingsBackend>,
    ) -> Self {
        Self {
            strings,
            backend,
            theme: UiTheme::default(),
            state: SettingsState::new(),
            connection: ConnectionTestState::Idle,
            request_preview: None,
            manual_model: String::new(),
            reveal_key: false,
            queried: std::collections::HashSet::new(),
            discovery_error: None,
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

    pub fn state(&self) -> &SettingsState {
        &self.state
    }

    /// 首次进入时补齐推荐模型（与切换厂商走同一条路：
    /// 按厂商查随包目录，而不是用硬编码名单）。
    pub fn prime_recommendations(&mut self) {
        self.refresh_recommendations();
    }

    pub fn state_mut(&mut self) -> &mut SettingsState {
        &mut self.state
    }

    pub fn ui(&mut self, ctx: &Context) {
        self.poll_job();
        // 补齐目录知识：Preset 推荐模型本身不带数据，需向 Core 查询。
        // 查不到就是"无目录数据"，界面如实显示而不是空白或编造（§X.16）
        self.enrich_models();

        egui::TopBottomPanel::top("umer-header").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(
                    RichText::new(self.text("wizard.title")).heading(),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let icon = if self.theme.mode == ThemeMode::Dark {
                        "☀"
                    } else {
                        "☾"
                    };
                    if ui.button(icon).clicked() {
                        let next = if self.theme.mode == ThemeMode::Dark {
                            ThemeMode::Light
                        } else {
                            ThemeMode::Dark
                        };
                        let ctx = ui.ctx().clone();
                        crate::theme::apply(&ctx, next, self.theme.density, self.theme.scale);
                        self.theme.mode = next;
                    }
                });
            });
            ui.add_space(2.0);
        });

        egui::TopBottomPanel::bottom("umer-actions").show(ctx, |ui| {
            ui.add_space(6.0);
            self.action_bar(ui);
            ui.add_space(6.0);
        });

        egui::SidePanel::left("umer-providers")
            .resizable(false)
            .exact_width(198.0)
            .show(ctx, |ui| {
                self.provider_list(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.provider_config(ui));
        });
    }

    fn text(&self, key: &str) -> String {
        let value = self.strings.get(key);
        if value.is_empty() {
            key.to_string()
        } else {
            value.to_string()
        }
    }

    // ---------- 左侧：厂商列表 ----------

    fn provider_list(&mut self, ui: &mut Ui) {
        let colors = semantic(self.theme.mode);
        ui.add_space(6.0);
        let mut search = self.state.search.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text(self.text("providers.search"))
                    .desired_width(ui.available_width()),
            )
            .changed()
        {
            self.state.search = search;
        }
        ui.add_space(6.0);

        let selected_id = self.state.selected_provider_id().to_string();
        let strings = &self.strings;
        let groups = self.state.grouped(|p| {
            let v = strings.get(p.name_key);
            if v.is_empty() {
                p.id.to_string()
            } else {
                v.to_string()
            }
        });

        if groups.iter().all(|(_, items)| items.is_empty()) {
            ui.label(RichText::new(self.text("providers.none")).color(colors.weak));
            return;
        }

        let mut pick: Option<&'static str> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (category, items) in groups {
                    if items.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(self.text(category.title_key()))
                            .small()
                            .color(colors.weak),
                    );
                    for preset in items {
                        if self.provider_row(ui, preset, preset.id == selected_id) {
                            pick = Some(preset.id);
                        }
                    }
                }
                ui.add_space(8.0);
            });

        if let Some(id) = pick {
            self.state.select_provider(id);
            self.connection = ConnectionTestState::Idle;
            self.request_preview = None;
            self.discovery_error = None;
            // 换厂商后按新厂商查目录拿推荐（而不是沿用上家的列表）
            self.refresh_recommendations();
        }
    }

    /// 按当前厂商向 Core 要推荐模型——**查随包目录，不是硬编码名字**。
    ///
    /// 目录里没有该厂商时列表会是空的，界面据此提示"刷新模型列表"或手填，
    /// 而不是显示一年前的型号。
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
            .map(|entry| {
                let profile = self.backend.model_info(&entry.model_id);
                ModelEntry::with_source(entry.model_id, profile, ModelSource::Catalog)
            })
            .collect();
        self.state.apply_recommendations(entries);
    }

    /// 侧栏里的一行厂商：徽标 + 名称 + 状态点。
    fn provider_row(&self, ui: &mut Ui, preset: &'static ProviderPreset, selected: bool) -> bool {
        let colors = semantic(self.theme.mode);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
        let painter = ui.painter();
        let rounding = egui::Rounding::same(6.0);
        if selected {
            painter.rect_filled(rect, rounding, colors.accent.gamma_multiply(0.18));
        } else if response.hovered() {
            painter.rect_filled(rect, rounding, ui.visuals().widgets.hovered.bg_fill);
        }

        let text_color = if selected {
            colors.accent
        } else {
            ui.visuals().text_color()
        };
        painter.text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            preset.badge,
            egui::FontId::proportional(11.0),
            colors.weak,
        );
        painter.text(
            rect.left_center() + egui::vec2(32.0, 0.0),
            egui::Align2::LEFT_CENTER,
            self.text(preset.name_key),
            egui::FontId::proportional(13.0),
            text_color,
        );
        // 状态点：本地部署用免密提示色，其余用"需要密钥"的灰点
        if preset.keyless {
            painter.circle_filled(rect.right_center() - egui::vec2(10.0, 0.0), 3.0, colors.ok);
        }
        response.clicked()
    }

    // ---------- 右侧：配置 ----------

    fn provider_config(&mut self, ui: &mut Ui) {
        let colors = semantic(self.theme.mode);
        let preset = self.state.preset();

        // 标题行：徽标 + 名称
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(preset.badge).strong().color(colors.accent));
            ui.add(egui::Label::new(
                RichText::new(self.text(preset.name_key)).heading(),
            ));
            if preset.keyless {
                ui.label(
                    RichText::new(self.text("providers.keyless"))
                        .small()
                        .color(colors.ok),
                );
            }
        });
        ui.label(
            RichText::new(self.text(preset.subtitle_key()))
                .small()
                .color(colors.weak),
        );
        ui.add_space(8.0);
        ui.separator();

        // 连接方式（协议）+ 文档链接
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(self.text("settings.provider.protocol"));
            let protocols = self.state.available_protocols();
            let current = self.state.protocol();
            let current_label = self.protocol_label(current);
            let mut picked: Option<ProtocolKind> = None;
            egui::ComboBox::from_id_salt("umer-protocol")
                .selected_text(current_label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for protocol in &protocols {
                        let label = self.protocol_label(*protocol);
                        if ui.selectable_label(*protocol == current, label).clicked() {
                            picked = Some(*protocol);
                        }
                    }
                });
            if let Some(protocol) = picked {
                self.state.set_protocol(protocol);
                self.request_preview = None;
            }
            if let Some(doc) = preset.doc_url {
                if ui
                    .link(RichText::new(self.text("providers.docs")).small())
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(doc));
                }
            }
        });

        // API Key
        ui.add_space(8.0);
        ui.label(self.text("settings.provider.api_key"));
        ui.horizontal(|ui| {
            let mut key = self.state.api_key().to_string();
            let hint = if self.state.keyless() {
                self.text("wizard.key.optional")
            } else {
                "sk-…".to_string()
            };
            let response = ui.add(
                egui::TextEdit::singleline(&mut key)
                    .password(!self.reveal_key)
                    .desired_width((ui.available_width() - 170.0).max(160.0))
                    .hint_text(hint),
            );
            if response.changed() {
                self.state.set_api_key(key);
            }
            let eye = if self.reveal_key { "🙈" } else { "👁" };
            if ui.button(eye).clicked() {
                self.reveal_key = !self.reveal_key;
            }
            if let Some(url) = preset.key_url {
                if ui.button(self.text("wizard.get_key")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });

        // Base URL + 实时预览
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(self.text("settings.provider.endpoint"));
            if !self.state.endpoint_is_default() {
                ui.label(
                    RichText::new(self.text("wizard.endpoint.custom"))
                        .small()
                        .color(colors.weak),
                );
                if ui
                    .link(RichText::new(self.text("wizard.endpoint.reset")).small())
                    .clicked()
                {
                    self.state.reset_endpoint();
                    self.request_preview = None;
                }
            }
        });
        let mut endpoint = self.state.endpoint().to_string();
        if ui
            .add(
                egui::TextEdit::singleline(&mut endpoint)
                    .desired_width(ui.available_width())
                    .hint_text("https://…/v1"),
            )
            .changed()
        {
            self.state.set_endpoint(endpoint);
            self.request_preview = None;
        }
        // 预览：由后端计算，保证与 Adapter 实际使用的 URL 一致
        if self.request_preview.is_none() {
            self.request_preview = self.backend.preview_request_url(&self.state.to_draft());
        }
        if let Some(preview) = &self.request_preview {
            // 必须 wrap：长 URL 不换行会把内容区撑宽，右侧控件被裁掉
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{} {}", self.text("providers.request_to"), preview))
                        .small()
                        .color(colors.weak),
                )
                .wrap(),
            );
        }

        // 模型列表
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.text("providers.models")).strong());
            if self.state.discovered_count() > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        self.text("wizard.models.discovered"),
                        self.state.discovered_count()
                    ))
                    .small()
                    .color(colors.weak),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = self.job.is_some();
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(
                            RichText::new(self.text("providers.refresh_models")).small(),
                        ),
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
                }
            });
        });
        if let Some(error) = &self.discovery_error {
            let message = format!("{} {}", self.text("providers.refresh_failed"), error);
            ui.add(egui::Label::new(RichText::new(message).small().color(colors.danger)).wrap());
        }
        ui.add_space(4.0);
        self.model_rows(ui);

        // 添加模型
        ui.add_space(6.0);
        let manual_hint = self.text("discovery.manual.add");
        let add_label = self.text("discovery.add");
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.manual_model)
                    .hint_text(manual_hint)
                    .desired_width(240.0),
            );
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || ui.button(add_label).clicked()) && !self.manual_model.trim().is_empty() {
                let id = self.manual_model.trim().to_string();
                let profile = self.lookup_profile(&id);
                self.state.add_model(ModelEntry::new(id, profile));
                self.manual_model.clear();
            }
        });

        // 模型知识 + 上下文证据（我们的模型数据体系在这里露出来）
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(4.0);
        self.model_knowledge(ui);
    }

    fn model_rows(&mut self, ui: &mut Ui) {
        let colors = semantic(self.theme.mode);
        if self.state.models().is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(self.text("wizard.models.empty")).color(colors.weak),
                )
                .wrap(),
            );
            return;
        }
        let selected = self.state.selected_model().map(str::to_string);
        let mut pick: Option<String> = None;
        let mut remove: Option<String> = None;
        let entries: Vec<ModelEntry> = self.state.models().to_vec();
        for entry in &entries {
            let is_selected = selected.as_deref() == Some(entry.model_id.as_str());
            let mut pending_pick = false;
            let mut pending_remove = false;
            ui.horizontal(|ui| {
                let width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
                if response.clicked() {
                    pending_pick = true;
                }
                let painter = ui.painter();
                let rounding = egui::Rounding::same(6.0);
                if is_selected {
                    painter.rect_filled(rect, rounding, colors.accent.gamma_multiply(0.14));
                    painter.rect_stroke(rect, rounding, egui::Stroke::new(1.0f32, colors.accent));
                } else if response.hovered() {
                    painter.rect_filled(rect, rounding, ui.visuals().widgets.hovered.bg_fill);
                }
                painter.text(
                    rect.left_center() + egui::vec2(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    entry.label(),
                    egui::FontId::proportional(13.0),
                    if is_selected {
                        colors.accent
                    } else {
                        ui.visuals().text_color()
                    },
                );
                // 上下文徽标 + 能力徽标
                let mut x = rect.right_center().x - 26.0;
                if let Some(profile) = &entry.profile {
                    for (kind, status) in profile.capabilities.iter().rev().take(3) {
                        if *status == CapabilityStatus::Supported {
                            let text = capability_short(*kind);
                            let galley = painter.layout_no_wrap(
                                text.to_string(),
                                egui::FontId::proportional(10.0),
                                colors.weak,
                            );
                            x -= galley.size().x + 6.0;
                            painter.text(
                                egui::pos2(x, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                text,
                                egui::FontId::proportional(10.0),
                                colors.weak,
                            );
                        }
                    }
                    if let Some(context) = profile.limits.context_window {
                        let text = short_tokens(context);
                        let galley = painter.layout_no_wrap(
                            text.clone(),
                            egui::FontId::proportional(10.0),
                            colors.weak,
                        );
                        x -= galley.size().x + 10.0;
                        painter.text(
                            egui::pos2(x, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::proportional(10.0),
                            colors.weak,
                        );
                    }
                }
                // 删除按钮
                let del_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.right_center().x - 12.0, rect.center().y),
                    egui::vec2(18.0, 18.0),
                );
                let del = ui.interact(
                    del_rect,
                    ui.id().with(&entry.model_id),
                    egui::Sense::click(),
                );
                painter.text(
                    del_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "×",
                    egui::FontId::proportional(13.0),
                    if del.hovered() {
                        colors.danger
                    } else {
                        colors.weak
                    },
                );
                if del.clicked() {
                    pending_remove = true;
                }
            });
            if pending_pick {
                pick = Some(entry.model_id.clone());
            }
            if pending_remove {
                remove = Some(entry.model_id.clone());
            }
        }
        if let Some(id) = pick {
            self.state.select_model(id);
        }
        if let Some(id) = remove {
            self.state.remove_model(&id);
        }
    }

    /// 模型知识面板：能力 / 价格 / 档位 / 上下文证据。
    fn model_knowledge(&mut self, ui: &mut Ui) {
        let colors = semantic(self.theme.mode);
        let Some(model_id) = self.state.selected_model().map(str::to_string) else {
            // 列表里有模型但尚未选中：给出明确指引（而不是重复"还没获取模型"）
            let key = if self.state.models().is_empty() {
                "wizard.models.empty"
            } else {
                "model.select_hint"
            };
            ui.add(egui::Label::new(RichText::new(self.text(key)).color(colors.weak)).wrap());
            return;
        };
        ui.label(RichText::new(&model_id).strong());

        // 能力
        let capabilities: Vec<(runtime_ui::CapabilityKind, CapabilityStatus)> = self
            .state
            .selected_profile()
            .map(|p| p.capabilities.clone())
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.text("model.capabilities"))
                    .small()
                    .color(colors.weak),
            );
            if capabilities.is_empty() {
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
            } else {
                for (kind, status) in &capabilities {
                    let (key, color) = match status {
                        CapabilityStatus::Supported => ("model.capability.supported", colors.ok),
                        CapabilityStatus::Unsupported => {
                            ("model.capability.unsupported", colors.weak)
                        }
                        CapabilityStatus::Partial => ("model.capability.partial", colors.accent),
                        CapabilityStatus::Unknown => ("model.capability.unknown", colors.weak),
                    };
                    let text = self.text(key);
                    ui.label(
                        RichText::new(format!("{kind:?}·{text}"))
                            .small()
                            .color(color),
                    );
                }
            }
        });

        // 价格
        let pricing = self
            .state
            .selected_profile()
            .and_then(|p| p.pricing.clone());
        if let Some(pricing) = pricing {
            let mut parts: Vec<String> = Vec::new();
            if let Some(v) = pricing.input_per_mtok {
                parts.push(format!("{} ${v:.4}/M", self.text("model.price.input")));
            }
            if let Some(v) = pricing.output_per_mtok {
                parts.push(format!("{} ${v:.4}/M", self.text("model.price.output")));
            }
            if let Some(v) = pricing.cached_input_per_mtok {
                parts.push(format!("{} ${v:.4}/M", self.text("model.price.cached")));
            }
            if !parts.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(self.text("model.pricing"))
                            .small()
                            .color(colors.weak),
                    );
                    ui.label(RichText::new(parts.join(" · ")).small());
                    ui.label(
                        RichText::new(self.text("model.pricing.notice"))
                            .small()
                            .color(colors.weak),
                    );
                });
            }
        }

        // 思考强度档位（未知就明说）
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.text("settings.provider.reasoning_effort"))
                    .small()
                    .color(colors.weak),
            );
            let efforts = self.state.supported_efforts().to_vec();
            if efforts.is_empty() {
                ui.label(
                    RichText::new(self.text("reasoning.effort.unknown"))
                        .small()
                        .color(colors.weak),
                );
            } else {
                for effort in &efforts {
                    ui.label(
                        RichText::new(effort.canonical_label())
                            .small()
                            .color(colors.accent),
                    );
                }
            }
        });

        // 上下文窗口 + 证据行 —— 我们的模型数据体系直接呈现
        ui.add_space(6.0);
        ui.label(self.text("model.context_window"));
        let mut context_input = self.state.context_override_input().to_string();
        if ui
            .add(
                egui::TextEdit::singleline(&mut context_input)
                    .desired_width(200.0)
                    .hint_text(self.text("model.context_window.hint")),
            )
            .changed()
        {
            self.state.set_context_override_input(context_input);
        }
        let evidence = self.state.context_evidence();
        self.evidence_line(ui, &evidence);
    }

    /// 那行"自动探测 … · 目录 … · 生效 …"。
    fn evidence_line(&self, ui: &mut Ui, evidence: &ContextWindowEvidence) {
        let colors = semantic(self.theme.mode);
        let mut parts: Vec<String> = Vec::new();
        let label = |key: &str| self.text(key);
        if let Some(v) = evidence.probed {
            parts.push(format!(
                "{} {}",
                label("model.context.probed"),
                thousands(v)
            ));
        }
        if let Some(v) = evidence.catalog {
            parts.push(format!(
                "{} {}",
                label("model.context.catalog"),
                thousands(v)
            ));
        }
        if let Some(v) = evidence.overridden {
            parts.push(format!(
                "{} {}",
                label("model.context.overridden"),
                thousands(v)
            ));
        }
        if evidence.has_any() {
            let value = evidence.effective.unwrap_or_default();
            parts.push(format!(
                "{} {}",
                label("model.context.effective"),
                thousands(value)
            ));
        }

        ui.horizontal_wrapped(|ui| {
            if parts.is_empty() {
                ui.label(
                    RichText::new(label("model.context.unknown"))
                        .small()
                        .color(colors.weak),
                );
                return;
            }
            ui.label(RichText::new(parts.join(" · ")).small().color(colors.weak));
            if evidence.conflict {
                ui.label(
                    RichText::new(label("model.context.conflict"))
                        .small()
                        .color(colors.danger),
                );
            }
        });
    }

    fn protocol_label(&self, protocol: ProtocolKind) -> String {
        let key = self
            .state
            .preset()
            .offering(protocol)
            .map(|o| o.protocol_label_key)
            .unwrap_or("protocol_label.openai_chat");
        self.text(key)
    }

    // ---------- 底部动作栏 ----------

    fn action_bar(&mut self, ui: &mut Ui) {
        let colors = semantic(self.theme.mode);
        let mut test = false;
        let mut save = false;
        ui.horizontal(|ui| {
            let can_save = self.state.is_ready();
            if ui
                .add_enabled(can_save, egui::Button::new(self.text("settings.save")))
                .clicked()
            {
                save = true;
            }
            if ui.button(self.text("connection.test")).clicked() {
                test = true;
            }
            match &self.connection {
                ConnectionTestState::Idle => {}
                ConnectionTestState::Testing => {
                    ui.add(egui::Spinner::new().size(13.0));
                    ui.label(RichText::new(self.text("connection.testing")).small());
                }
                ConnectionTestState::Ok { latency_ms } => {
                    let text = self.text("connection.ok");
                    ui.label(
                        RichText::new(format!("✓ {text} · {latency_ms}ms"))
                            .small()
                            .color(colors.ok),
                    );
                }
                ConnectionTestState::Failed { reason_key } => {
                    let text = self.text(reason_key);
                    ui.label(RichText::new(text).small().color(colors.danger));
                }
            }
            // 校验问题（非阻断项静默，阻断项红色）
            for issue in self.state.blocking_issues() {
                let text = self.text(issue);
                ui.label(RichText::new(text).small().color(colors.danger));
            }
        });
        if test {
            self.start_connection_test();
        }
        if save {
            // 无宿主回调时的演示语义：把就绪状态显式呈现
            self.connection = ConnectionTestState::Ok { latency_ms: 0 };
        }
    }

    /// 从后端查询模型知识（目录里没有则 None，界面如实显示"无数据"）。
    fn lookup_profile(&self, model_id: &str) -> Option<UiModelInfo> {
        self.backend.model_info(model_id)
    }

    /// 给列表里还没有知识的模型补上目录数据（含能力徽标、上下文、价格、档位）。
    ///
    /// 只在缺失时查询一次（`queried` 记录已查过的 ID），避免每帧打后端；
    /// 查不到就保持 None，界面据此显示"目录中没有该模型的数据"。
    fn enrich_models(&mut self) {
        let missing: Vec<String> = self
            .state
            .models()
            .iter()
            .filter(|m| m.profile.is_none() && !self.queried.contains(&m.model_id))
            .map(|m| m.model_id.clone())
            .collect();
        for model_id in missing {
            self.queried.insert(model_id.clone());
            if let Some(profile) = self.backend.model_info(&model_id) {
                self.state
                    .add_model(ModelEntry::new(model_id, Some(profile)));
            }
        }
        // 尚未选模型时默认选中第一个：避免右侧一片空白，也少一次点击
        if self.state.selected_model().is_none() {
            if let Some(first) = self.state.models().first() {
                let first = first.model_id.clone();
                self.state.select_model(first);
            }
        }
    }

    // ---------- 后台动作 ----------

    fn start_connection_test(&mut self) {
        self.connection = ConnectionTestState::Testing;
        let backend = self.backend.clone();
        let draft = self.state.to_draft();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("umer-ui-connection".into())
            .spawn(move || {
                let _ = tx.send(JobResult::Connection(backend.test_connection(&draft)));
            })
            .ok();
        self.job = Some(Job { rx });
    }

    fn start_discovery(&mut self) {
        let backend = self.backend.clone();
        let draft = self.state.to_draft();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("umer-ui-discovery".into())
            .spawn(move || {
                let _ = tx.send(JobResult::Discovery(backend.discover(&draft)));
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
                match result {
                    Ok(found) => {
                        let entries: Vec<ModelEntry> = found
                            .iter()
                            .map(|entry| {
                                ModelEntry::new(
                                    entry.model_id.clone(),
                                    self.backend.model_info(&entry.model_id),
                                )
                            })
                            .collect();
                        self.state.apply_discovery(entries);
                    }
                    Err(_) => {
                        // 发现失败不阻断：用户可以手动添加（§36）
                    }
                }
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
            }
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 自愈：eframe 会按系统主题覆盖 visuals，被改回就重新应用
        let want = match self.theme.mode {
            ThemeMode::Dark => egui::Theme::Dark,
            ThemeMode::Light => egui::Theme::Light,
        };
        if ctx.theme() != want || (ctx.pixels_per_point() - self.theme.scale).abs() > 0.001 {
            crate::theme::apply(ctx, self.theme.mode, self.theme.density, self.theme.scale);
        }
        self.ui(ctx);
    }
}

/// 能力的中文短名（徽标用，避免长词把行撑开）。
fn capability_short(kind: runtime_ui::CapabilityKind) -> &'static str {
    use runtime_ui::CapabilityKind::*;
    match kind {
        Text => "文",
        Vision => "视",
        Audio => "听",
        Video => "影",
        Reasoning => "思",
        ToolCall => "工",
        ParallelToolCall => "并",
        StructuredOutput => "结",
        JsonMode => "J",
        Streaming => "流",
        Embeddings => "向",
    }
}

/// 上下文窗口徽标：按数值习惯选择进制——
/// 128000 → "128K"（十进制取整）、65536 → "64K"、1048576 → "1M"。
fn short_tokens(value: u64) -> String {
    const K: u64 = 1024;
    const M: u64 = 1024 * 1024;
    if value >= 1_000_000 && value % 1_000_000 == 0 {
        return format!("{}M", value / 1_000_000);
    }
    if value >= M && value % M == 0 {
        return format!("{}M", value / M);
    }
    if value >= 8_000 && value % 1_000 == 0 {
        return format!("{}K", value / 1_000);
    }
    if value >= 8 * K && value % K == 0 {
        return format!("{}K", value / K);
    }
    value.to_string()
}

/// 128000 → "128,000"（证据行用千位分隔，便于比对）。
fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_shorthand_is_readable() {
        assert_eq!(short_tokens(65_536), "64K");
        assert_eq!(short_tokens(1_048_576), "1M");
        assert_eq!(short_tokens(128_000), "128K");
        assert_eq!(short_tokens(8_192), "8K");
        assert_eq!(short_tokens(512), "512");
    }

    #[test]
    fn thousands_separator_matches_reference_style() {
        assert_eq!(thousands(1_024_000), "1,024,000");
        assert_eq!(thousands(256_000), "256,000");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
    }

    #[test]
    fn every_category_has_a_title_key() {
        for preset in runtime_ui::BUILTIN_PRESETS {
            let key = preset.subtitle_key();
            assert!(key.starts_with("category."), "{key}");
        }
    }
}
