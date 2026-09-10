//! egui/eframe 参考实现（总案 §38 §41.5）。
//!
//! **冻结的是 UISpec 数据契约；本 crate 整体是可替换的视觉层。**
//!
//! 结构约束（§37：UI 与 Core 分离的制度保障）：
//! - 只依赖 `runtime-ui` 的数据类型（SettingsPage / DiscoverySession /
//!   Strings / SettingsBackend）
//! - 不读文件、不发网络、不碰凭据——一切动作经 `SettingsBackend`
//! - 网络/凭据动作放到工作线程，渲染线程永不被阻塞（egui 立即模式的硬要求）
//!
//! 渲染参数（§38）：Theme / Language / Scale / Density 四项全部生效。
//! 中文回退字体从系统加载（微软雅黑 / 苹方 / Noto Sans CJK），不打包字体——
//! 打包 CJK 字体会让"体积小"的目标直接破产。

#![forbid(unsafe_code)]

pub mod app;
pub mod fonts;
pub mod theme;

pub use app::SettingsApp;
pub use theme::{Density, ThemeMode, UiTheme};

use runtime_ui::SettingsBackend;
/// 打开设置窗口的全部参数（总案 §38 的四个渲染参数 + 页面 + 接线）。
pub struct SettingsWindowParams {
    pub title: String,
    pub theme: ThemeMode,
    pub language: &'static str,
    pub scale: f32,
    pub density: Density,
    pub page: runtime_ui::SettingsPage,
    pub backend: std::sync::Arc<dyn SettingsBackend>,
}

impl Default for SettingsWindowParams {
    fn default() -> Self {
        Self {
            title: "Universal Model Runtime".into(),
            theme: ThemeMode::Light,
            language: "zh-CN",
            scale: 1.0,
            density: Density::Cozy,
            page: runtime_ui::SettingsPage::provider_settings(),
            backend: std::sync::Arc::new(backend::NullBackend),
        }
    }
}

mod backend {
    use runtime_ui::{BackendError, ConnectionReport, SettingsBackend, SettingsDraft};

    /// 无动作后端：窗口可以独立打开（动作按钮报"未接线"）。
    pub struct NullBackend;

    impl SettingsBackend for NullBackend {
        fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
            Err(BackendError::new(
                "connection.failed",
                "no backend wired into this window",
            ))
        }
        fn discover(
            &self,
            _: &SettingsDraft,
        ) -> Result<Vec<runtime_ui::UiModelEntry>, BackendError> {
            Err(BackendError::new("discovery.empty", "no backend wired"))
        }
    }
}

/// 打开设置窗口（阻塞至窗口关闭；宿主应在自己的线程调用）。
pub fn open_settings_window(params: SettingsWindowParams) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 880.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title(params.title.clone()),
        ..Default::default()
    };
    eframe::run_native(
        &params.title,
        options,
        Box::new(move |cc| {
            let strings = runtime_ui::Strings::builtin(params.language)
                .unwrap_or_else(|| runtime_ui::Strings::builtin("zh-CN").expect("zh-CN exists"));
            let mut app = SettingsApp::new(params.page, strings, params.backend);
            app.apply_theme(&cc.egui_ctx, params.theme, params.density, params.scale);
            app.install_system_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
