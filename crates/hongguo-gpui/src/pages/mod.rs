//! 目的地页面集合：侧栏 12 目的地里除首页沉浸流（`home/`）与壳内详情页
//! （`detail/`）之外的整页视图，与 Tauri `src/pages/` 的页面一一对应。
//! 每个页面一个模块，对外的入口是各自的 `*View` 实体（壳在 `shell.rs`
//! 装配、切换目的地）。

use gpui_kit::prelude::*;

pub(crate) mod browse;
pub(crate) mod collections;
pub(crate) mod creator;
pub(crate) mod follows;
pub(crate) mod history;
pub(crate) mod liked;
pub(crate) mod list_common;
pub(crate) mod merge;
pub(crate) mod new_drama;
pub(crate) mod rank;
pub(crate) mod reservations;
pub(crate) mod settings;
pub(crate) mod storage;
pub(crate) mod tasks;

/// 页面卡面：Tauri Card 的 GPUI 对齐形态（圆角 12 + 边框 + 卡面底色 +
/// p-6），磁盘清理 / 一键合并 / 设置等设置型页面共用。
pub(crate) fn card(cx: &gpui_kit::App) -> gpui_kit::Div {
    use gpui_kit::component::{ActiveTheme, v_flex};
    let theme = cx.theme();
    v_flex()
        .w_full()
        .rounded(gpui_kit::px(12.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.group_box)
        .p_6()
        .gap_4()
}

/// 在文件管理器中定位到文件（Tauri opener `reveal_item_in_dir` 同款；
/// 一键合并的产物定位与下载管理的「打开文件夹」共用）。
pub(crate) fn reveal_in_folder(path: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result: std::io::Result<std::process::Child> = {
        log::warn!("[Shell] 打开文件夹在 {path} 所在平台未实现");
        return;
    };
    if let Err(e) = result {
        log::warn!("[Shell] 打开文件夹失败: {e}");
    }
}
