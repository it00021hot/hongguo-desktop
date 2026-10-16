//! 在进程内把窗口**已渲染的场景**存成 PNG（`feature = "screenshot"`）。
//!
//! 存在的理由：macOS 上 `screencapture` 抓别的进程窗口要屏幕录制授权，CI/
//! 无头环境拿不到，抓到的只有壁纸（现象：窗口在,截出来是桌面）。GPUI 自带
//! `Window::render_to_image`——把最近一帧的场景渲到离屏纹理再读回像素，不
//! 经过窗口服务器，不需要任何系统授权。这是图形链路唯一可靠的取证手段。
//!
//! 用法：`HONGGUO_SHOT=/tmp/shot.png HONGGUO_SHOT_COUNT=6 make gpui-screenshot`
//! （或 `cargo run -p hongguo-gpui --features screenshot`），产物为
//! `/tmp/shot.png.1`…`N`，间隔 `HONGGUO_SHOT_INTERVAL_MS`（默认 1500ms）。

use std::time::Duration;

use gpui_kit::{AnyWindowHandle, App};

/// 环境变量起点火：设了 `HONGGUO_SHOT` 就按节奏抓若干张。
/// `feed` 是壳根的 HomeView 实体（HONGGUO_SHOT_PAUSE 取证暂停用）。
pub(crate) fn install(
    window: AnyWindowHandle,
    feed: gpui_kit::Entity<crate::shell::Shell>,
    cx: &mut App,
) {
    let Ok(path) = std::env::var("HONGGUO_SHOT") else {
        return;
    };
    let count = env_usize("HONGGUO_SHOT_COUNT", 6);
    let interval = Duration::from_millis(env_usize("HONGGUO_SHOT_INTERVAL_MS", 1500) as u64);
    let start_delay = Duration::from_millis(env_usize("HONGGUO_SHOT_START_MS", 8_000) as u64);
    // HONGGUO_SHOT_PAUSE=1：抓帧前暂停播放并把控制条/互动栏的可见窗拉长
    // ——控制条 3s 自隐、互动栏随它显隐，无人值守抓帧永远抓不到 UI；
    // 要验 UI 视觉（图标/布局）就开这个。
    let pause_ui = std::env::var("HONGGUO_SHOT_PAUSE").is_ok();
    // HONGGUO_SHOT_PANEL=downloads：抓帧前打开下载面板（验任务行 UI）；
    // =picker：打开选集浮层系列剧 tab（验季卡片/预约按钮）。
    let panel = std::env::var("HONGGUO_SHOT_PANEL").unwrap_or_default();

    cx.spawn(async move |cx| {
        cx.background_executor().timer(start_delay).await;
        if pause_ui {
            feed.update(cx, |shell, cx| shell.debug_pause_player(cx));
        }
        match panel.as_str() {
            "downloads" => {
                feed.update(cx, |shell, cx| shell.debug_open_downloads(cx));
            }
            "picker" => {
                feed.update(cx, |shell, cx| shell.debug_open_picker(cx));
            }
            "comments" => {
                feed.update(cx, |shell, cx| shell.debug_open_comments(cx));
            }
            "mention" => {
                feed.update(cx, |shell, cx| shell.debug_open_mention(cx));
            }
            "login" => {
                feed.update(cx, |shell, cx| shell.debug_open_login(cx));
            }
            _ => {}
        }
        for i in 1..=count {
            let shot = window.update(cx, |_, window, cx| {
                // 必须先强制重绘：窗口被遮挡/失焦时 AppKit 会暂停绘制，
                // rendered_frame 会停在上一帧——直接取图会拿到陈旧画面
                //（表现为每次抓图都一模一样、内容停在启动瞬间）。
                // 返回值是「要不要清 arena」的提示，抓帧路径不看它
                let _ = window.draw(cx);
                window
                    .render_to_image()
                    .map_err(|e| format!("渲染取图失败: {e}"))
            });
            match shot {
                Ok(Ok(image)) => {
                    let out = format!("{path}.{i}.png");
                    match image.save(&out) {
                        Ok(()) => log::info!(
                            "[Shot] 第 {i}/{count} 张已保存 {out}（{}×{}）",
                            image.width(),
                            image.height()
                        ),
                        Err(e) => log::warn!("[Shot] 写盘失败 {out}: {e}"),
                    }
                }
                Ok(Err(e)) => log::warn!("[Shot] 第 {i} 张失败: {e}"),
                Err(e) => {
                    log::warn!("[Shot] 窗口已关闭（{e}），停止抓图");
                    return;
                }
            }
            cx.background_executor().timer(interval).await;
        }
    })
    .detach();
}

fn env_usize(key: &str, fallback: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}
