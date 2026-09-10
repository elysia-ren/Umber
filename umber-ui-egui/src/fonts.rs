//! 中文回退字体：从系统加载，不打包（打包 CJK 字体会增加 10–20 MB，
//! 与"体积小"的指标直接冲突）。
//!
//! 候选按平台排列，命中即止；全部未命中时保留 egui 默认字体
//! （CJK 显示为方框，但功能不受影响——极少数精简系统才走到这步）。

use std::path::PathBuf;

use egui::{Context, FontData, FontDefinitions, FontFamily};

#[cfg(windows)]
fn candidates() -> Vec<PathBuf> {
    let fonts = std::env::var("WINDIR")
        .map(|w| PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\Fonts"));
    vec![
        fonts.join("msyh.ttc"), // 微软雅黑
        fonts.join("msyh.ttf"),
        fonts.join("simhei.ttf"), // 黑体
        fonts.join("simsun.ttc"), // 宋体
    ]
}

#[cfg(target_os = "macos")]
fn candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"),
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf"),
        PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
        PathBuf::from("/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc"),
    ]
}

/// 尝试安装 CJK 回退字体。返回是否命中系统字体。
pub fn install_cjk_fallback(ctx: &Context) -> bool {
    for path in candidates() {
        if let Ok(bytes) = std::fs::read(&path) {
            let mut defs = FontDefinitions::default();
            defs.font_data
                .insert("umer-cjk".into(), FontData::from_owned(bytes));
            // 插到最前：CJK 字形优先命中，拉丁字形回落到 egui 内置
            if let Some(family) = defs.families.get_mut(&FontFamily::Proportional) {
                family.insert(0, "umer-cjk".into());
            }
            if let Some(family) = defs.families.get_mut(&FontFamily::Monospace) {
                family.push("umer-cjk".into());
            }
            ctx.set_fonts(defs);
            return true;
        }
    }
    false
}
