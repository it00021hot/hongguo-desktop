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
    // =picker：打开选集浮层系列剧 tab（验季卡片/预约按钮）；
    // =suggest（配 HONGGUO_PAGE=browse）：往搜索框灌关键词，拍真实联想
    // 下拉（300ms 防抖 → 真实接口，连封面缩略图一起验）。
    let panel = std::env::var("HONGGUO_SHOT_PANEL").unwrap_or_default();

    cx.spawn(async move |cx| {
        cx.background_executor().timer(start_delay).await;
        if pause_ui {
            feed.update(cx, |shell, cx| shell.debug_pause_player(cx));
        }
        for i in 1..=count {
            // 面板开关放在本回合里：`suggest` 要往输入框灌值，得有 Window。
            if i == 1 {
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
                    "suggest" => {
                        let _ = window.update(cx, |_, window, cx| {
                            feed.update(cx, |shell, cx| {
                                shell.debug_fill_browse_keyword(window, cx);
                            });
                        });
                    }
                    // 长页面整页入镜的补充手段：窗口最多涨到屏幕高，再往下
                    // 的卡片只能滚出来（HONGGUO_SHOT_SCROLL=<像素>）。
                    "scroll" => {
                        let dy = std::env::var("HONGGUO_SHOT_SCROLL")
                            .ok()
                            .and_then(|v| v.parse::<f32>().ok())
                            .unwrap_or(800.0);
                        let _ = window.update(cx, |_, window, cx| {
                            // 先派一次鼠标移动把命中测试摆到页面中部，再滚
                            let size = window.viewport_size();
                            let center = gpui_kit::point(size.width / 2.0, size.height / 2.0);
                            window.dispatch_event(
                                gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                                    position: center,
                                    pressed_button: None,
                                    modifiers: Default::default(),
                                }),
                                cx,
                            );
                            window.dispatch_event(
                                gpui_kit::PlatformInput::ScrollWheel(gpui_kit::ScrollWheelEvent {
                                    position: center,
                                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                        gpui_kit::px(0.0),
                                        gpui_kit::px(-dy),
                                    )),
                                    modifiers: Default::default(),
                                    touch_phase: gpui_kit::TouchPhase::Started,
                                }),
                                cx,
                            );
                        });
                    }
                    // 老板键：走真实保存路径（注册 → 落库；冲突则当场拦截）。
                    // 抓帧拍通知条，日志拍库里的值。
                    "boss-set" => {
                        let _ = window.update(cx, |_, window, cx| {
                            feed.update(cx, |shell, cx| shell.debug_set_boss_key(window, cx));
                        });
                    }
                    // 竖屏：按当前在播档位收窄窗口（拍竖条形态 + 紧凑条）。
                    "portrait" => {
                        let _ = window.update(cx, |_, window, cx| {
                            feed.update(cx, |shell, cx| shell.debug_enter_portrait(window, cx));
                        });
                    }
                    // 竖屏铺满档：撑满工作区高（验档位几何与按钮态）。
                    "portrait-full" => {
                        let _ = window.update(cx, |_, window, cx| {
                            feed.update(cx, |shell, cx| shell.debug_portrait_full(window, cx));
                        });
                    }
                    // 退出确认：投一个真 WM_CLOSE（等同点 × / Alt+F4），
                    // 期望窗口**没关**且确认框在抓帧里可见。
                    "close-test" => {
                        let _ = window.update(cx, |_, window, cx| {
                            if let Some(hwnd) = crate::win32::hwnd_of(window) {
                                log::info!("[CloseProbe] 投递 WM_CLOSE");
                                crate::win32::post_close(hwnd);
                            }
                            let _ = cx;
                        });
                    }
                    _ => {}
                }
            }
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
        // HONGGUO_BOSS_PROBE=<accelerator>：老板键链路探针——合成该组合键，
        // 系统派 WM_HOTKEY → 轮询藏窗/显窗。按键靠**启动时的设置注册**生效，
        // 验的是「设置里存过 → 启动注册 → 系统认下 → 藏显」这条真实链。
        // 结果只落日志；抓帧循环之后跑，隐藏窗口不影响已拍完的图。
        #[cfg(target_os = "windows")]
        if let Ok(spec) = std::env::var("HONGGUO_BOSS_PROBE") {
            use crate::boss_key::accel;
            log::info!("[BossProbe] 探针组合 {spec}（应为启动注册生效的那个）");
            let hk = accel::parse(&spec).expect("探针组合必然可解析");
            let hwnd = window
                .update(cx, |_, window, _| crate::win32::hwnd_of(window))
                .ok()
                .flatten();
            let initial = hwnd.map(crate::win32::is_window_visible);
            log::info!("[BossProbe] 初始可见={initial:?}（隐藏启动时 false，属启动方式而非功能）");
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            crate::win32::send_key_chord(hk.mods, hk.vk);
            cx.background_executor()
                .timer(Duration::from_millis(700))
                .await;
            if let Some(hwnd) = hwnd {
                log::info!(
                    "[BossProbe] 第一次按下后 可见={}（期望与初始相反）",
                    crate::win32::is_window_visible(hwnd)
                );
                crate::win32::send_key_chord(hk.mods, hk.vk);
                cx.background_executor()
                    .timer(Duration::from_millis(700))
                    .await;
                log::info!(
                    "[BossProbe] 第二次按下后 可见={}（期望回到初始）",
                    crate::win32::is_window_visible(hwnd)
                );
            }
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
