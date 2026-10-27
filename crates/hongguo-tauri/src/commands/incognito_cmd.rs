//! 隐身模式命令：光标轮询驱动窗口藏显（鼠标脱离 → 隐藏+暂停，回到 → 重现）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tauri::AppHandle;

use hongguo_common::AppResult;

// ---------------------------------------------------------------- 隐身模式

/// 隐身可见性事件载荷（`incognito-visibility`）：前端据此暂停/恢复音频
/// （窗口藏起来前要让音频先停）。与前端 `EVENT_PAYLOADS['incognito-visibility']`
/// 两端镜像，改字段同一 commit 改另一端。
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IncognitoVisibilityEvent {
    visible: bool,
}

/// 隐身模式全局开关与轮询代次。
///
/// 隐身的语义是**鼠标脱离窗口 → 隐藏+暂停，鼠标回到窗口区域 → 窗口自动
/// 重现**（不是最小化、不用点 Dock）。隐藏的窗口收不到任何鼠标事件，
/// 「回来了」只能靠**系统级光标位置轮询**判定——这是桌面端做这件事的
/// 唯一路径：窗口隐藏后位置不动，全局光标一旦重新落进
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
        // 只有隐身**真的开着过**才需要收回窗口。挂载同步（隐身本来就没开）
        // 绝不能 show+focus：播放页每次重挂载（剧终接力/自动连播后的导航）
        // 都会重发一次 setIncognito(false)，把最小化或被遮挡的窗口弹到最
        // 前面——听剧听得好好的窗口突然蹦到前台（失焦状态下
        // /player 重挂必抢前台）。
        if was_on && let Some(w) = active_window(&app) {
            let _ = w.show();
            let _ = w.set_focus();
            let _ = app.emit_to(
                w.label(),
                "incognito-visibility",
                IncognitoVisibilityEvent { visible: true },
            );
        }
        return Ok(());
    }
    // 代次 +1 让可能仍在跑的旧轮询自杀，再起一条新循环
    let generation = INCOGNITO_GEN.fetch_add(1, Ordering::Release) + 1;
    // 同步 command 在主线程上执行，那里没有 Tokio 线程上下文，裸
    // `tokio::spawn` 会 panic（且 scheme/IPC 回调跨 objc 边界不能 unwind，
    // 直接 abort 闪退）——必须走 tauri 的全局运行时
    tauri::async_runtime::spawn(async move {
        incognito_watch(app, generation).await;
    });
    Ok(())
}

/// 隐身轮询的目标窗口：主窗口（小屏播放是同一窗口，无需再裁决）。
fn active_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    use tauri::Manager;
    app.get_webview_window("main")
}

async fn incognito_watch(app: AppHandle, generation: u64) {
    use tauri::Emitter;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(INCOGNITO_POLL_MS)).await;
        if !INCOGNITO_ON.load(Ordering::Acquire)
            || INCOGNITO_GEN.load(Ordering::Acquire) != generation
        {
            return;
        }
        let Some(win) = active_window(&app) else {
            return;
        };
        // 最小化是用户的显式动作，隐身不得插手：最小化后 is_visible 变
        // false，而光标多半还留在原窗口矩形里——不跳过的话每轮都会判成
        // 「鼠标回来了」把窗口 show 回来，表现为「隐身开着就最小化不了」。
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
                IncognitoVisibilityEvent { visible: true },
            );
        } else {
            // 鼠标走了：先通知前端暂停（音频不能跟着窗口一起消失前还在响），
            // 给一小段送达窗口再藏
            let _ = app.emit_to(
                win.label(),
                "incognito-visibility",
                IncognitoVisibilityEvent { visible: false },
            );
            tokio::time::sleep(std::time::Duration::from_millis(INCOGNITO_PAUSE_GRACE_MS)).await;
            if !INCOGNITO_ON.load(Ordering::Acquire) {
                continue;
            }
            let _ = win.hide();
        }
    }
}
