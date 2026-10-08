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

// ---------------------------------------------------------------- 小屏播放

/// 小屏播放退出时要恢复的窗口几何（进入那一刻的快照，物理坐标）。
#[derive(Clone, Copy)]
struct SavedGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
}

/// 进入小屏前的窗口几何。std 锁 + 中毒恢复（本文件既定纪律：持锁内只有
/// 整体读写，恢复使用即可，不能让一次后台 panic 连环炸掉后续命令）。
static MINI_RESTORE: std::sync::Mutex<Option<SavedGeometry>> = std::sync::Mutex::new(None);

fn saved_geometry() -> std::sync::MutexGuard<'static, Option<SavedGeometry>> {
    MINI_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 小屏尺寸（对齐 hgplayer 的 Ud/Vd 常量）与落角边距。hgplayer 底部留 72
/// 是给 Windows 任务栏的；macOS 工作区已扣掉 Dock，给 16 即可。
const MINI_W: f64 = 480.0;
const MINI_H: f64 = 270.0;
const MINI_MARGIN: f64 = 16.0;
/// 主窗口平时的最小尺寸（tauri.conf.json 的 minWidth/minHeight）。进小屏
/// 必须先降到小屏尺寸以下，否则缩窗被最小值 clamp 住纹丝不动——
/// hgplayer 进小屏前同样先 `WindowSetMinSize`。
const MAIN_MIN: (f64, f64) = (1024.0, 680.0);

/// 藏/显 macOS 原生红绿灯的三颗圆点（小屏模式用）。
///
/// 刻意**不动 styleMask**：tao 的 `set_decorations` 是整体替换 styleMask，
/// 会把 Overlay 全出血依赖的 `FullSizeContentView` 位一起抹掉——标题条重新
/// 占位 28px、整个 app 内容被顶下去（外框实测 840→868）。而且它走 GCD 主
/// 队列异步块，事后用 `set_title_bar_style` 补位是内联立即执行，永远先于
/// 排队的替换落地、补不过它（探针实测 inner=840/outer=868 复证）。所以直接
/// 对三颗 `NSWindowButton` setHidden：窗口结构零变化，尺寸语义全程稳定。
/// hgplayer 小屏连标题栏都没有，480×270 的画面上再叠三颗圆点纯属多余；
/// Windows 本就 decorations:false，无需此操作。
fn set_traffic_lights_hidden(app: &AppHandle, hidden: bool) {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use tauri::Manager;

    // NSWindow/NSButton 的方法不是线程安全的，落到主线程执行
    let app = app.clone();
    let _ = app.run_on_main_thread({
        let app = app.clone();
        move || {
            let Some(win) = app.get_webview_window("main") else {
                return;
            };
            let Ok(ptr) = win.ns_window() else {
                return;
            };
            let ns_window = unsafe { &*(ptr as *const NSWindow) };
            for kind in [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ] {
                if let Some(btn) = ns_window.standardWindowButton(kind) {
                    btn.setHidden(hidden);
                }
            }
        }
    });
}

/// 进入小屏播放（对齐 hgplayer 的「小屏播放」按钮 ng()/rw()）：
/// **同一个窗口**缩成 480×270、落到工作区右下角——不另开窗口、不藏
/// 主窗、不暂停，`<video>` 元素原地不动、播放零中断。
///
/// 旧实现（独立置顶小窗 + 隐藏主窗）整套移除：主窗藏了但它的视频还在
/// 响，两路声音叠着播，窗口管理也全乱了套——那是没有对照第三方的
/// 杜撰设计。
#[tauri::command]
pub fn enter_mini_screen(app: AppHandle) -> AppResult<()> {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };

    // 幂等：已在小屏（有快照）就不重复缩，重复点按钮不该把大窗几何
    // 覆盖成 480×270（否则退出小屏恢复的也是小屏尺寸）
    if saved_geometry().is_some() {
        return Ok(());
    }

    // 快照用 inner_size（内容尺寸）：exit 的 set_size 语义就是「设内容
    // 尺寸」，拿外框当内容还原会每圈漂移。红绿灯改为只 setHidden 后
    // （见 set_traffic_lights_hidden）styleMask 全程不动，macOS 下外框
    // 与内容恒等，与无框的 Windows/Linux 行为一致，快照精确往返。
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return Ok(());
    };
    *saved_geometry() = Some(SavedGeometry {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        maximized: win.is_maximized().unwrap_or(false),
    });

    // macOS：藏掉原生红绿灯（只 setHidden 三颗圆点，不动 styleMask，
    // 理由见 set_traffic_lights_hidden）；退出时在 exit_mini_screen 里显回。
    #[cfg(target_os = "macos")]
    set_traffic_lights_hidden(&app, true);

    let mon = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let (left, top, right, bottom) = work_area_logical(mon.as_ref());
    // 小工作区（外接竖屏等）放不下就压到放得下为止
    let w = MINI_W.min(right - left - 2.0 * MINI_MARGIN);
    let h = MINI_H.min(bottom - top - 2.0 * MINI_MARGIN);

    let _ = win.set_min_size(Some(tauri::LogicalSize::new(w, h)));
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    // 手动 min/max 而非 clamp：极小工作区下 min>max 时 clamp 会 panic
    let x = (right - w - MINI_MARGIN).max(left);
    let y = (bottom - h - MINI_MARGIN).max(top);
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    let _ = win.set_focus();
    Ok(())
}

/// 退出小屏：恢复进入前的窗口几何与最小尺寸约束。
///
/// 没有快照（应用启动就在小屏、或未进过）就只恢复 minSize 约束——
/// 尺寸位置保持现状，不瞎动。
#[tauri::command]
pub fn exit_mini_screen(app: AppHandle) -> AppResult<()> {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    // 原生红绿灯无条件显回（macOS 小屏时被藏了，见 enter_mini_screen）——
    // 放在「无快照提前 return」之前，异常路径也要还原。
    #[cfg(target_os = "macos")]
    set_traffic_lights_hidden(&app, false);
    let saved = saved_geometry().take();
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(MAIN_MIN.0, MAIN_MIN.1)));
    let Some(g) = saved else {
        return Ok(());
    };
    // g.width/height 是进入时的内容尺寸（inner_size），与 set_size 语义
    // 一致，见 enter_mini_screen 里的快照注释
    let _ = win.set_size(tauri::PhysicalSize::new(g.width, g.height));
    let _ = win.set_position(tauri::PhysicalPosition::new(g.x, g.y));
    if g.maximized {
        let _ = win.maximize();
    }
    Ok(())
}

/// 窗口置顶开关（对齐 hgplayer 的 De.pinned → WindowSetAlwaysOnTop）。
///
/// **窗口级**的会话内状态：大屏置顶后进小屏依然置顶，退出小屏也不清——
/// 「钉住」是用户对整个窗口的意图，与窗口大小无关。
#[tauri::command]
pub fn set_always_on_top(app: AppHandle, enabled: bool) -> AppResult<()> {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_always_on_top(enabled);
    }
    Ok(())
}

/// 显示器工作区（逻辑坐标，`left/top/right/bottom`）。
///
/// monitor 的 work_area 是物理像素，HiDPI 下要除以缩放系数才是窗口 API
/// 用的逻辑坐标。拿不到显示器时给一版 14" MacBook 的保守估值——宁小勿
/// 出界，小窗被夹得小一点总比伸到屏幕外强。
fn work_area_logical(mon: Option<&tauri::Monitor>) -> (f64, f64, f64, f64) {
    match mon {
        Some(m) => {
            let sf = m.scale_factor();
            let wa = m.work_area();
            (
                wa.position.x as f64 / sf,
                wa.position.y as f64 / sf,
                (wa.position.x + wa.size.width as i32) as f64 / sf,
                (wa.position.y + wa.size.height as i32) as f64 / sf,
            )
        }
        None => (0.0, 40.0, 1440.0, 875.0),
    }
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
/// 开 = 启动光标轮询（盯着主窗口）；关 = 停轮询，并立刻带回可能正处在
/// 隐藏态的窗口。
#[tauri::command]
pub fn set_incognito(app: AppHandle, enabled: bool) -> AppResult<()> {
    use tauri::Emitter;

    let was_on = INCOGNITO_ON.swap(enabled, Ordering::Release);
    if !enabled {
        // 关掉的一瞬可能正处于隐身隐藏中：立刻恢复可见，否则窗口没人管。
        // 之前开着（隐身暂停过）才补 visible:true——前端据此续播，与
        // 「鼠标回来」同一语义；本来就没开时发它只会是无意义的空事件。
        if let Some(w) = active_window(&app) {
            let _ = w.show();
            let _ = w.set_focus();
            if was_on {
                let _ = app.emit_to(
                    w.label(),
                    "incognito-visibility",
                    serde_json::json!({ "visible": true }),
                );
            }
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

/// 隐身轮询的目标窗口：主窗口（小屏播放是同一窗口，无需再裁决）。
fn active_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    use tauri::Manager;
    app.get_webview_window("main")
}

async fn incognito_watch(app: AppHandle, gen: u64) {
    use tauri::Emitter;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(INCOGNITO_POLL_MS)).await;
        if !INCOGNITO_ON.load(Ordering::Acquire) || INCOGNITO_GEN.load(Ordering::Acquire) != gen {
            return;
        }
        let Some(win) = active_window(&app) else {
            return;
        };
        // 最小化是用户的显式动作，隐身不得插手：最小化后 is_visible 变
        // false，而光标多半还留在原窗口矩形里——不跳过的话每轮都会判成
        // 「鼠标回来了」把窗口 show 回来，表现为「隐身开着就最小化不了」
        // （2026-10-07 实测）。
        if win.is_minimized().unwrap_or(false) {
            continue;
        }
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
