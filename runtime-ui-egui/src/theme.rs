//! 设计 token（总案 §38 的 Theme / Scale / Density 三个渲染参数）。
//!
//! 现代感的来源就是这里：一套克制的灰阶 + 单一强调色 + 统一圆角。
//! egui 默认样式是"开发工具脸"，可替换性契约要求宿主能整层换掉——
//! 所以 token 只在这一处定义，换皮不动布局代码。

use egui::{Color32, Context, Rounding, Style, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    Compact,
    #[default]
    Cozy,
}

/// 渲染参数包（总案 §38）：Theme + Density + Scale。
#[derive(Debug, Clone, Copy)]
pub struct UiTheme {
    pub mode: ThemeMode,
    pub density: Density,
    pub scale: f32,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            density: Density::Cozy,
            scale: 1.0,
        }
    }
}

/// 单一强调色：明暗两套共用，保证品牌一致性。
pub const ACCENT: Color32 = Color32::from_rgb(0x3B, 0x82, 0xF6);

struct Palette {
    text: Color32,
    text_weak: Color32,
    panel_bg: Color32,
    window_bg: Color32,
    extreme_bg: Color32,
    stroke: Color32,
    selection: Color32,
    danger: Color32,
    ok: Color32,
}

const DARK: Palette = Palette {
    text: Color32::from_rgb(0xE8, 0xEA, 0xED),
    text_weak: Color32::from_rgb(0x9A, 0xA0, 0xA6),
    panel_bg: Color32::from_rgb(0x1B, 0x1D, 0x23),
    window_bg: Color32::from_rgb(0x20, 0x23, 0x2B),
    extreme_bg: Color32::from_rgb(0x14, 0x16, 0x1B),
    stroke: Color32::from_rgb(0x32, 0x36, 0x40),
    selection: Color32::from_rgba_premultiplied(0x3B, 0x82, 0xF6, 0x66),
    danger: Color32::from_rgb(0xEF, 0x44, 0x44),
    ok: Color32::from_rgb(0x22, 0xC5, 0x5E),
};

const LIGHT: Palette = Palette {
    text: Color32::from_rgb(0x1A, 0x1C, 0x1F),
    text_weak: Color32::from_rgb(0x6B, 0x72, 0x80),
    panel_bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    window_bg: Color32::from_rgb(0xF6, 0xF7, 0xF9),
    extreme_bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    stroke: Color32::from_rgb(0xD9, 0xDD, 0xE3),
    selection: Color32::from_rgba_premultiplied(0x3B, 0x82, 0xF6, 0x55),
    danger: Color32::from_rgb(0xDC, 0x26, 0x26),
    ok: Color32::from_rgb(0x16, 0xA3, 0x4A),
};

impl ThemeMode {
    fn palette(self) -> Palette {
        match self {
            ThemeMode::Dark => DARK,
            ThemeMode::Light => LIGHT,
        }
    }
}

impl Density {
    fn item_spacing(self) -> Vec2 {
        match self {
            Density::Compact => Vec2::new(6.0, 4.0),
            Density::Cozy => Vec2::new(10.0, 8.0),
        }
    }

    fn button_padding(self) -> Vec2 {
        match self {
            Density::Compact => Vec2::new(8.0, 3.0),
            Density::Cozy => Vec2::new(12.0, 6.0),
        }
    }

    fn margin(self) -> egui::Margin {
        match self {
            Density::Compact => egui::Margin::same(10.0),
            Density::Cozy => egui::Margin::same(16.0),
        }
    }

    fn rounding(self) -> Rounding {
        match self {
            Density::Compact => Rounding::same(4.0),
            Density::Cozy => Rounding::same(8.0),
        }
    }
}

/// 把 token 应用到 ctx。设计目标是：布局代码零改动即可整体换肤。
pub fn apply(ctx: &Context, mode: ThemeMode, density: Density, scale: f32) {
    let p = mode.palette();
    ctx.set_pixels_per_point(scale.max(0.5));

    let mut style = Style {
        visuals: egui::Visuals::default(),
        ..Style::default()
    };
    style.visuals.dark_mode = mode == ThemeMode::Dark;
    style.visuals.panel_fill = p.panel_bg;
    style.visuals.window_fill = p.window_bg;
    style.visuals.extreme_bg_color = p.extreme_bg;
    style.visuals.override_text_color = Some(p.text);
    style.visuals.selection.bg_fill = p.selection;
    style.visuals.selection.stroke = egui::Stroke::new(1.0f32, ACCENT);
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.window_rounding = Rounding::same(10.0);
    style.visuals.menu_rounding = Rounding::same(8.0);
    let rounding = density.rounding();
    style.visuals.widgets.noninteractive.rounding = rounding;
    style.visuals.widgets.inactive.rounding = rounding;
    style.visuals.widgets.hovered.rounding = rounding;
    style.visuals.widgets.active.rounding = rounding;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0f32, p.stroke);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0f32, p.stroke);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0f32, ACCENT.gamma_multiply(0.6));
    style.visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.3);
    style.spacing.item_spacing = density.item_spacing();
    style.spacing.button_padding = density.button_padding();
    style.spacing.window_margin = density.margin();
    style.spacing.menu_margin = density.margin();
    style.text_styles = std::collections::BTreeMap::from([
        (egui::TextStyle::Heading, egui::FontId::proportional(18.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(14.0)),
        (egui::TextStyle::Button, egui::FontId::proportional(14.0)),
        (egui::TextStyle::Small, egui::FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(13.0)),
    ]);
    ctx.set_style(style);
}

/// 语义色（应用层取用，保证与主题一致）。
pub fn semantic(mode: ThemeMode) -> SemanticColors {
    let p = mode.palette();
    SemanticColors {
        danger: p.danger,
        ok: p.ok,
        weak: p.text_weak,
        accent: ACCENT,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SemanticColors {
    pub danger: Color32,
    pub ok: Color32,
    pub weak: Color32,
    pub accent: Color32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_tokens_differ_and_stay_positive() {
        assert!(Density::Compact.item_spacing() != Density::Cozy.item_spacing());
        assert!(Density::Compact.button_padding() != Density::Cozy.button_padding());
        assert!(Density::Cozy.item_spacing().x > 0.0);
    }

    #[test]
    fn modes_have_distinct_palettes() {
        assert_ne!(DARK.panel_bg, LIGHT.panel_bg);
        assert_ne!(DARK.text, LIGHT.text);
    }

    #[test]
    fn apply_runs_headless() {
        // 不开窗口也能完整走一遍样式应用（无头可测性）
        let ctx = Context::default();
        apply(&ctx, ThemeMode::Dark, Density::Compact, 1.25);
        apply(&ctx, ThemeMode::Light, Density::Cozy, 1.0);
        assert!((ctx.pixels_per_point() - 1.0).abs() < f32::EPSILON);
    }
}
