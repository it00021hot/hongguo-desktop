//! 系统交互：选目录、打开目录、窗口关闭的退出确认。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::error::AppResult;

/// 窗口关闭拦截的「前端就绪」闸门：前端界面挂载完成后置位。
///
/// 之前就拦截的话，前端一旦没加载出来（白屏/崩溃），被拦下的关闭请求
/// 永远没人应答，窗口就关不掉了——没 ready 就放行，宁可不确认也不能困住用户。
pub type WindowCloseGate = Arc<AtomicBool>;

/// 选择下载目录。
#[tauri::command]
pub fn select_folder(app: AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(|p| p.to_string()))
}

/// 打开某剧的任务所在目录。
#[tauri::command]
pub fn open_folder(app: AppHandle, state: State<'_, AppState>, series_id: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let queue = state.queue();
    let path = queue
        .of_series(&series_id)
        .into_iter()
        .map(|t| t.file_path)
        .find(|p| !p.is_empty())
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("剧集 {series_id} 没有已下载文件"))
        })?;

    app.opener()
        .reveal_item_in_dir(std::path::Path::new(&path))
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
}

/// 用系统默认浏览器打开外部网址。
///
/// 只给固定的几个白名单网址用（设置页的 ffmpeg 安装指引），
/// 不接受任意输入拼 URL，避免变成「前端想打开什么就打开什么」。
#[tauri::command]
pub fn open_external_page(app: AppHandle, page: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    const ALLOWED: &[&str] = &[
        "https://www.gyan.dev/ffmpeg/builds/",
        "https://ffmpeg.org/download.html",
    ];
    if !ALLOWED.contains(&page.as_str()) {
        return Err(crate::error::AppError::InvalidArgs(format!(
            "不允许打开的网址: {page}"
        )));
    }
    app.opener()
        .open_url(page, None::<&str>)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
}

/// 前端界面挂载完成：此后窗口关闭请求转交前端弹确认框，不再直接退出。
///
/// 退出是危险动作——正在跑的下载任务会被掐断，所以自绘 ×/Alt+F4/任务栏
/// 关闭一律先问一声。lib.rs 的 CloseRequested 拦截只在本命令调用过之后生效。
#[tauri::command]
pub fn mark_window_ready(ready: State<'_, WindowCloseGate>) -> AppResult<()> {
    ready.store(true, Ordering::Release);
    Ok(())
}

/// 用户在退出确认框里选了「退出」：结束整个应用。
#[tauri::command]
pub fn exit_app(app: AppHandle) -> AppResult<()> {
    app.exit(0);
    Ok(())
}

/// 小窗播放：为当前这一集开一个置顶的独立小窗，主窗口随之隐藏。
///
/// 为什么不用浏览器原生画中画：PiP 窗口的尺寸/行为归系统管（默认偏小、
/// 也藏不了主窗口），自建窗口尺寸可控（竖屏短剧给竖屏窗口）、能挂隐身
/// 模式这类自定义行为。hgplayer（Wails）的小窗同理。
///
/// 小窗生命周期：关闭（自绘 × / `close_mini_window` / 随应用退出）时在
/// `Destroyed` 里把主窗口带回来并给主窗发 `mini-closed`——主窗播放器
/// 据此把进度对齐到小窗刚写到后端的位置。
#[tauri::command]
pub fn open_mini_window(app: AppHandle, series_id: String, vid_index: u32) -> AppResult<()> {
    use tauri::{Emitter, Manager};

    // 已开过：让小窗换到这一集（整页重载，进度链路走同一套 play/resumeAt），
    // 并补一次主窗隐藏（用户可能又把主窗点出来了）
    if let Some(mini) = app.get_webview_window("mini") {
        let script = format!("location.replace('/mini?series={series_id}&index={vid_index}')");
        let _ = mini.eval(&script);
        let _ = mini.set_focus();
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.hide();
        }
        return Ok(());
    }

    let url = tauri::WebviewUrl::App(format!("/mini?series={series_id}&index={vid_index}").into());
    let mini = tauri::WebviewWindowBuilder::new(&app, "mini", url)
        .title("红果短剧 · 小窗")
        // 起手按竖屏短剧给竖窗（常态）；起播后 fit_mini_window 按视频实际
        // 宽高比校正——横屏剧自动变横窗，视频铺满没有黑边（hgplayer 同款）
        .inner_size(424.0, 768.0)
        // 下限只保控件摆得下；高度不设竖屏值，否则横屏窗被顶出黑边
        .min_inner_size(320.0, 220.0)
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .always_on_top(true)
        .build()?;

    // 小窗销毁（×、返回主窗、应用退出）→ 主窗回来 + 通知主窗对齐进度
    let handle = app.clone();
    mini.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if let Some(main) = handle.get_webview_window("main") {
                let _ = main.show();
                let _ = main.set_focus();
            }
            let _ = handle.emit_to("main", "mini-closed", ());
        }
    });

    // 开屏落角（画中画惯例位：右下角），不压任务栏/Dock——工作区由系统给
    if let Some(mon) = mini
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())
    {
        let sf = mon.scale_factor();
        let wa = mon.work_area();
        // 与 inner_size(424×768) 对应的逻辑尺寸；fit_mini_window 校正比例后
        // 角位依然成立（右下角锚定，改尺寸只会向左上伸缩）
        let (w, h) = (424.0_f64, 768.0_f64);
        const MARGIN: f64 = 16.0;
        let x = wa.position.x as f64 / sf + wa.size.width as f64 / sf - w - MARGIN;
        let y = wa.position.y as f64 / sf + wa.size.height as f64 / sf - h - MARGIN;
        let _ = mini.set_position(tauri::LogicalPosition::new(x, y));
    }

    // 小窗就位后再藏主窗：先藏后建的话，建窗失败用户就两窗全无
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.hide();
    }
    Ok(())
}

/// 关闭小窗、回到主窗口（主窗恢复统一由小窗的 `Destroyed` 事件完成）。
#[tauri::command]
pub fn close_mini_window(app: AppHandle) -> AppResult<()> {
    use tauri::Manager;
    if let Some(mini) = app.get_webview_window("mini") {
        let _ = mini.close();
    }
    Ok(())
}

/// 小窗尺寸对齐视频宽高比（起播后由前端在 metadata 就绪时调用）。
///
/// 窗口比例 = 视频比例，视频铺满、零黑边（hgplayer 小窗形态）；竖屏剧
/// 与横屏剧各自拿到合适的长宽。目标面积观感恒定（竖屏约 424×768 那一档），
/// 并压进当前显示器的工作区。视频尺寸未知（0）时不动。
#[tauri::command]
pub fn fit_mini_window(app: AppHandle, video_width: u32, video_height: u32) -> AppResult<()> {
    use tauri::Manager;
    if video_width == 0 || video_height == 0 {
        return Ok(());
    }
    let Some(mini) = app.get_webview_window("mini") else {
        return Ok(());
    };
    let ratio = f64::from(video_width) / f64::from(video_height);

    // 逻辑坐标下的屏幕可用区（HiDPI 下 monitor 物理尺寸要除以缩放系数）
    let monitor = mini
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let (sw, sh) = monitor
        .map(|m| {
            let s = m.size();
            let sf = m.scale_factor();
            (s.width as f64 / sf, s.height as f64 / sf)
        })
        .unwrap_or((1512.0, 982.0));

    let (w, h) = if ratio >= 1.0 {
        // 横屏：高按屏幕四成上下、上限 520（再大就不叫小窗了）
        let h = (sh * 0.42).clamp(240.0, 520.0);
        (h * ratio, h)
    } else {
        // 竖屏：宽按屏幕四分之一上下、上限 440
        let w = (sw * 0.28).clamp(320.0, 440.0);
        (w, w / ratio)
    };
    let (w, h) = (w.min(sw * 0.9), h.min(sh * 0.9));
    let _ = mini.set_size(tauri::LogicalSize::new(w, h));
    Ok(())
}

// ---------------------------------------------------------------- 隐身模式

/// 隐身模式全局开关与轮询代次。
///
/// 隐身的语义是**鼠标脱离窗口 → 隐藏+暂停，鼠标回到窗口区域 → 窗口自动
/// 重现**（不是最小化、不用点 Dock）。隐藏的窗口收不到任何鼠标事件，
/// 「回来了」只能靠**系统级光标位置轮询**判定——这是桌面端做这件事的
/// 唯一路径（hgplayer 同理）：窗口隐藏后位置不动，全局光标一旦重新落进
/// 它的矩形，立刻 show，体验上就是「鼠标回来窗口就回来了」。
static INCOGNITO_ON: AtomicBool = AtomicBool::new(false);
static INCOGNITO_GEN: AtomicU64 = AtomicU64::new(0);

/// 隐身轮询间隔：再快肉眼无感，再慢「回来」会迟半拍。
const INCOGNITO_POLL_MS: u64 = 120;
/// 隐藏前给前端留的暂停送达窗口：音频多响 100ms 就是 100ms 的暴露。
const INCOGNITO_PAUSE_GRACE_MS: u64 = 80;

/// 开关隐身模式（播放器的 Eye 按钮）。
///
/// 开 = 启动光标轮询（以「活动的窗口」为目标：有小窗是小窗，否则主窗）；
/// 关 = 停轮询，并立刻带回可能正处在隐藏态的窗口。
#[tauri::command]
pub fn set_incognito(app: AppHandle, enabled: bool) -> AppResult<()> {
    INCOGNITO_ON.store(enabled, Ordering::Release);
    if !enabled {
        // 关掉的一瞬可能正处于隐身隐藏中：立刻恢复可见，否则窗口没人管
        if let Some(w) = active_window(&app) {
            let _ = w.show();
            let _ = w.set_focus();
        }
        return Ok(());
    }
    // 代次 +1 让可能仍在跑的旧轮询自杀，再起一条新循环
    let gen = INCOGNITO_GEN.fetch_add(1, Ordering::Release) + 1;
    // 同步 command 在主线程上执行，那里没有 Tokio 线程上下文，裸
    // `tokio::spawn` 会 panic（且 scheme/IPC 回调跨 objc 边界不能 unwind，
    // 直接 abort 闪退）——必须走 tauri 的全局运行时
    tauri::async_runtime::spawn(async move {
        incognito_watch(app, gen).await;
    });
    Ok(())
}

/// 活动的窗口：有小窗是小窗，否则主窗（与 Dock 恢复同一裁决规则）。
fn active_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    use tauri::Manager;
    app.get_webview_window("mini").or_else(|| app.get_webview_window("main"))
}

async fn incognito_watch(app: AppHandle, gen: u64) {
    use tauri::Emitter;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(INCOGNITO_POLL_MS)).await;
        if !INCOGNITO_ON.load(Ordering::Acquire)
            || INCOGNITO_GEN.load(Ordering::Acquire) != gen
        {
            return;
        }
        let Some(win) = active_window(&app) else { return };
        let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
            continue;
        };
        let inside = match mouse_position::mouse_position::Mouse::get_mouse_position() {
            mouse_position::mouse_position::Mouse::Position { x, y } => {
                x >= pos.x
                    && y >= pos.y
                    && x < pos.x + size.width as i32
                    && y < pos.y + size.height as i32
            }
            // 光标位置拿不到：本轮不动，绝不基于未知状态藏窗
            _ => continue,
        };
        let visible = win.is_visible().unwrap_or(true);
        if inside == visible {
            continue;
        }
        if inside {
            // 鼠标回来了：窗口原地重现（show 不夺焦点就不打断用户手上的事）
            let _ = win.show();
            let _ = app.emit_to(
                win.label(),
                "incognito-visibility",
                serde_json::json!({ "visible": true }),
            );
        } else {
            // 鼠标走了：先通知前端暂停（音频不能跟着窗口一起消失前还在响），
            // 给一小段送达窗口再藏
            let _ = app.emit_to(
                win.label(),
                "incognito-visibility",
                serde_json::json!({ "visible": false }),
            );
            tokio::time::sleep(std::time::Duration::from_millis(INCOGNITO_PAUSE_GRACE_MS)).await;
            if !INCOGNITO_ON.load(Ordering::Acquire) {
                continue;
            }
            let _ = win.hide();
        }
    }
}
