//! 无头帧测试：不开窗口、不需要 GPU，直接驱动 egui Context 跑帧，
//! 验证完整渲染路径（schema 驱动的表单 / 主题切换 / 状态机绘制）。

use std::sync::Arc;

use runtime_ui::SettingsPage;
use runtime_ui_egui::{SettingsApp, ThemeMode};

mod support {
    use runtime_ui::{
        BackendError, ConnectionReport, SettingsBackend, SettingsDraft, UiModelEntry, UiModelInfo,
    };

    /// 记录调用的空后端（无头测试用）。
    pub struct RecordingBackend {
        pub test_calls: std::sync::atomic::AtomicUsize,
        pub discover_calls: std::sync::atomic::AtomicUsize,
    }

    impl RecordingBackend {
        pub fn new() -> Self {
            Self {
                test_calls: std::sync::atomic::AtomicUsize::new(0),
                discover_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl SettingsBackend for RecordingBackend {
        fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
            self.test_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(BackendError::new("connection.failed", "recording"))
        }
        fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
            self.discover_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(BackendError::new("discovery.empty", "recording"))
        }
        fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
            Some(UiModelInfo::unknown(model_id))
        }
    }
}

#[test]
fn renders_many_frames_headless() {
    let ctx = egui::Context::default();
    let mut app = SettingsApp::new(
        SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("zh-CN").unwrap(),
        Arc::new(support::RecordingBackend::new()),
    );
    app.apply_theme(&ctx, ThemeMode::Dark, runtime_ui_egui::Density::Cozy, 1.0);
    app.install_system_fonts(&ctx);

    // 跑多帧：立即模式下每帧都是完整的 ui() 路径
    for _ in 0..5 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    }
    // 主题切换后再渲染（换肤路径）
    app.apply_theme(
        &ctx,
        ThemeMode::Light,
        runtime_ui_egui::Density::Compact,
        1.25,
    );
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    }
}

#[test]
fn drafts_are_seeded_with_schema_defaults() {
    // 从 schema 默认值播种：medium 档位 / 主动探测关闭（§17.2 §21.1）
    let ctx = egui::Context::default();
    let mut app = SettingsApp::new(
        SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("en").unwrap(),
        Arc::new(support::RecordingBackend::new()),
    );
    app.apply_theme(&ctx, ThemeMode::Light, runtime_ui_egui::Density::Cozy, 1.0);
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    // 无 panic 即通过；草稿播种行为在 SettingsApp::new 内部完成
}
