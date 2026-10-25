//! 窗口管理命令：小屏播放（enter/exit_mini_screen）、竖屏播放
//! （enter/exit/fit_portrait_screen + set_portrait_full）、置顶、红绿灯藏显。

use tauri::AppHandle;

use hongguo_common::AppResult;

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

/// 小屏尺寸与落角边距。底部边距按 72
/// 预留 Windows 任务栏的量；macOS 工作区已扣掉 Dock，给 16 即可。
const MINI_W: f64 = 480.0;
const MINI_H: f64 = 270.0;
const MINI_MARGIN: f64 = 16.0;
/// 小屏下的最小尺寸：倍速紧贴时长 + 紧凑间距后，控制条一行固定内容约
/// 352px——360 即「播放控件不换行」的下限（用户口径：还可以再小）；
/// 高 180 是 16:9 附近的观感下限（控制条悬浮在画面上，高度不挤控件）。
const MINI_MIN: (f64, f64) = (360.0, 180.0);
/// 主窗口平时的最小尺寸（tauri.conf.json 的 minWidth/minHeight）＝小屏
/// 同款（#5：常态也允许缩到迷你尺寸，进出小屏只差几何与控制条形态）。
const MAIN_MIN: (f64, f64) = (MINI_W, MINI_H);

// ---------------------------------------------------------------- 无边框播放

/// 播放模式「窗口化无边框」开着的标记（前端控制栏开关，会话态）：小屏
/// 退出显回红绿灯时要不要重新藏，由它说了算——否则无边框播放进出一次
/// 小屏，圆点就在纯画面上复活了。
static FRAMELESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 藏/显 macOS 原生红绿灯（无边框播放模式的开关；小屏的藏显走同一实现）。
///
/// 刻意**不动 styleMask**（缘由见 [`set_traffic_lights_hidden`] 的注释）：
/// 只 setHidden 三颗圆点，窗口圆角/阴影/边缘缩放全部保留。Windows/Linux
/// 本就 decorations:false，命令照常成功、无实际操作。
#[tauri::command]
pub fn set_traffic_lights_visible(app: AppHandle, visible: bool) -> AppResult<()> {
    use std::sync::atomic::Ordering;
    FRAMELESS.store(!visible, Ordering::Release);
    #[cfg(target_os = "macos")]
    set_traffic_lights_hidden(&app, !visible);
    // Windows/Linux 没有红绿灯；AppHandle 是 Tauri 注入参数，非 macOS 目标显式忽略。
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

/// 藏/显 macOS 原生红绿灯的三颗圆点（小屏模式用）。
///
/// 刻意**不动 styleMask**：tao 的 `set_decorations` 是整体替换 styleMask，
/// 会把 Overlay 全出血依赖的 `FullSizeContentView` 位一起抹掉——标题条重新
/// 占位 28px、整个 app 内容被顶下去（外框实测 840→868）。而且它走 GCD 主
/// 队列异步块，事后用 `set_title_bar_style` 补位是内联立即执行，永远先于
/// 排队的替换落地、补不过它（探针实测 inner=840/outer=868 复证）。所以直接
/// 对三颗 `NSWindowButton` setHidden：窗口结构零变化，尺寸语义全程稳定。
/// 小屏窗口连标题栏都没有，480×270 的画面上再叠三颗圆点纯属多余；
/// Windows 本就 decorations:false，无需此操作。
#[cfg(target_os = "macos")]
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

/// 进入小屏播放（对齐官方客户端的「小屏播放」行为）：
/// **同一个窗口**缩成 480×270、落到工作区右下角——不另开窗口、不藏
/// 主窗、不暂停，`<video>` 元素原地不动、播放零中断。
///
/// 旧实现（独立置顶小窗 + 隐藏主窗）整套移除：主窗藏了但它的视频还在
/// 响，两路声音叠着播，窗口管理也全乱了套——那是闭门杜撰的设计。
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

    // 常态此刻还是窗口真身：先把当前几何精确写进常态槽位（防抖线程
    // 500ms 一拍，紧接着进形态就关窗会把最后一次拖动落在档外）
    crate::window_state::save(&win);

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

    let _ = win.set_min_size(Some(tauri::LogicalSize::new(MINI_MIN.0, MINI_MIN.1)));
    // 上次小窗的几何有效（≥MINI_MIN 且落在任一显示器工作区内）就原样
    // 回去——三种窗态各记各的槽位，切形态互不覆盖；没有记忆（首次进
    // 小屏）才回 480×270 落角默认
    if !crate::window_state::apply_saved(&win, crate::window_state::Mode::Mini, MINI_MIN) {
        let mon = win
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten());
        let (left, top, right, bottom) = work_area_logical(mon.as_ref());
        // 小工作区（外接竖屏等）放不下就压到放得下为止
        let w = MINI_W.min(right - left - 2.0 * MINI_MARGIN);
        let h = MINI_H.min(bottom - top - 2.0 * MINI_MARGIN);
        // 手动 min/max 而非 clamp：极小工作区下 min>max 时 clamp 会 panic
        let x = (right - w - MINI_MARGIN).max(left);
        let y = (bottom - h - MINI_MARGIN).max(top);
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
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
    // 原生红绿灯按模式显回（macOS 小屏时被藏了，见 enter_mini_screen）——
    // 放在「无快照提前 return」之前，异常路径也要还原。无边框播放开着时
    // 保持隐藏：圆点不该在纯画面上复活（FRAMELESS 标记见其定义处）。
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::Ordering;
        set_traffic_lights_hidden(&app, FRAMELESS.load(Ordering::Acquire));
    }
    // 退出前先把当前小窗几何精确写进小屏槽位（此刻窗态还是小屏，save 按
    // 形态路由）：防抖线程 500ms 一拍，拖完立刻退出时最后一次拖动还悬在
    // 档外——重进小屏就会回到拖动前的旧位置。
    crate::window_state::save(&win);
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

/// 窗口置顶开关（走 WindowSetAlwaysOnTop）。
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

// ---------------------------------------------------------------- 竖屏播放

// 竖屏播放：窗口收窄成按
// 视频宽高比算宽的竖条，竖屏视频刚好包住画面
// 不留两侧黑边。两档——普通档保持当前窗口高（最多工作区高的 70%）；
// 铺满档（portraitFull）撑满工作区高并居中。
//
// 三处刻意口径：最小宽度取 360（沿用「控制条一行不换行」的 360 下限，同
// MINI_MIN）；标题栏高度按 0 算（本壳无框，macOS overlay 内容满窗）；
// 普通档高度最多吃工作区高的 70%（参照实现整窗天生竖条尺寸，「保持
// 当前高」没有巨条问题，本壳从大窗/最大化进竖屏会得到撑满的巨条——
// 用户实测「默认尺寸太大」）。
// 其余对齐：最大化状态进竖屏直接进铺满档；普通档在**当前窗口 x** 上
// 居中（不跳屏幕中心）；宽高比钳宽溢出工作区时反推高度（参照实现同
// 款，不再回钳工作区高）。

/// 竖屏退出时要恢复的窗口几何（进入那一刻的快照，语义同 MINI_RESTORE）。
static PORTRAIT_RESTORE: std::sync::Mutex<Option<SavedGeometry>> = std::sync::Mutex::new(None);

/// 铺满档退出时要还原的普通档几何（参照实现同款：只快照 x/y/w/h）。
static PORTRAIT_FULL_RESTORE: std::sync::Mutex<Option<(i32, i32, u32, u32)>> =
    std::sync::Mutex::new(None);

/// 竖屏下的最小尺寸（高 400 同参照实现；宽见上面模块注释）。
const PORTRAIT_MIN: (f64, f64) = (360.0, 400.0);

/// 普通档高度上限 = 工作区高 × 此系数（刻意差异第三处，见模块注释）。
const PORTRAIT_DEFAULT_HEIGHT_RATIO: f64 = 0.7;

fn portrait_restore() -> std::sync::MutexGuard<'static, Option<SavedGeometry>> {
    PORTRAIT_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn portrait_full_restore() -> std::sync::MutexGuard<'static, Option<(i32, i32, u32, u32)>> {
    PORTRAIT_FULL_RESTORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 当前窗态（window_state 落盘路由用）：竖屏亮着优先于小屏——正常流程
/// 进小屏前会先退竖屏（前端 enterMini 的守卫），真叠起来时亮在眼前的
/// 也是竖屏条。
pub(crate) fn current_mode() -> crate::window_state::Mode {
    if portrait_restore().is_some() {
        crate::window_state::Mode::Portrait
    } else if saved_geometry().is_some() {
        crate::window_state::Mode::Mini
    } else {
        crate::window_state::Mode::Normal
    }
}

/// 按视频宽高比重算竖屏几何并落窗（参照实现逐步同款）。
fn fit_portrait(win: &tauri::WebviewWindow, aspect: f64, full: bool) {
    let mon = win.current_monitor().ok().flatten();
    let (left, top, right, bottom) = work_area_logical(mon.as_ref());
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return;
    };
    let sf = win.scale_factor().unwrap_or(1.0);
    let cur_x = pos.x as f64 / sf;
    let cur_y = pos.y as f64 / sf;
    let cur_w = size.width as f64 / sf;
    let cur_h = size.height as f64 / sf;
    let wa_w = right - left;
    let wa_h = bottom - top;

    // 高：铺满档撑满工作区；普通档保持当前高，但最多吃工作区高的 70%
    //（大窗进竖屏不再得到撑满巨条；系数见 PORTRAIT_DEFAULT_HEIGHT_RATIO）
    let mut h = if full {
        wa_h
    } else {
        cur_h.min((wa_h * PORTRAIT_DEFAULT_HEIGHT_RATIO).round())
    };
    // 宽 =（高 − 标题栏）× 宽高比；溢出工作区时钳宽反推高
    let mut w = (h * aspect).round();
    if w > wa_w {
        w = wa_w;
        h = (w / aspect).round();
    }
    w = w.max(PORTRAIT_MIN.0);
    // x：铺满档居中于工作区；普通档居中于**当前窗口**（夹进工作区，
    // 手动 min/max 而非 clamp：极小工作区下 min>max 时 clamp 会 panic）
    let x = if full {
        left + ((wa_w - w) / 2.0).round().max(0.0)
    } else {
        (cur_x + ((cur_w - w) / 2.0).round())
            .max(left)
            .min(right - w)
    };
    let y = if full { top } else { cur_y };
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
}

/// 进入竖屏播放。`aspect` 是视频宽高比（w/h），由前端按本壳口径
/// 算好传入（非竖屏比回落 9:16）。已在竖屏中幂等。
#[tauri::command]
pub fn enter_portrait_screen(app: AppHandle, aspect: f64) -> AppResult<()> {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    // 幂等 + 参数守卫（已进竖屏或 aspect 非正都拒——参照实现的守卫口径；
    // mini 由前端保证；
    // NaN/0/负数/无穷都拒——NaN 一旦漏进几何计算，set_size 的行为不可预期）
    if portrait_restore().is_some() || !aspect.is_finite() || aspect <= 0.0 {
        return Ok(());
    }
    // 常态此刻还是窗口真身（小屏叠加进来时小屏入口已落过常态槽，幂等
    // 无害）：先把当前几何精确写进常态槽位再切形态，理由同 enter_mini_screen
    crate::window_state::save(&win);
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return Ok(());
    };
    let maximized = win.is_maximized().unwrap_or(false);
    *portrait_restore() = Some(SavedGeometry {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        maximized,
    });
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(
        PORTRAIT_MIN.0,
        PORTRAIT_MIN.1,
    )));
    // 最大化进竖屏：退最大化直接进铺满档（参照实现同款）；
    // 普通进入优先回上次竖屏槽位（三种窗态各记各的），没有记忆才按比例
    // 现算（首次进竖屏）
    if maximized {
        let _ = win.unmaximize();
        fit_portrait(&win, aspect, true);
    } else if !crate::window_state::apply_saved(
        &win,
        crate::window_state::Mode::Portrait,
        PORTRAIT_MIN,
    ) {
        fit_portrait(&win, aspect, false);
    }
    Ok(())
}

/// 竖屏中换集/换流导致视频宽高比变化后的重适配。
/// 铺满与否由前端镜像态传入；不在竖屏中则忽略。
#[tauri::command]
pub fn fit_portrait_screen(app: AppHandle, aspect: f64, full: bool) -> AppResult<()> {
    use tauri::Manager;
    if portrait_restore().is_none() || !aspect.is_finite() || aspect <= 0.0 {
        return Ok(());
    }
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    fit_portrait(&win, aspect, full);
    Ok(())
}

/// 铺满档切换：普通→铺满先快照当前几何再撑满；铺满→普通
/// 还原快照（没有快照——最大化直接进来的——就从当前几何按普通档重算）。
#[tauri::command]
pub fn set_portrait_full(app: AppHandle, aspect: f64, full: bool) -> AppResult<()> {
    use tauri::Manager;
    if portrait_restore().is_none() {
        return Ok(());
    }
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    if full {
        let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
            return Ok(());
        };
        *portrait_full_restore() = Some((pos.x, pos.y, size.width, size.height));
        fit_portrait(&win, aspect, true);
    } else {
        let saved = portrait_full_restore().take();
        match saved {
            Some((x, y, w, h)) => {
                let _ = win.set_size(tauri::PhysicalSize::new(w, h));
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
            }
            None => fit_portrait(&win, aspect, false),
        }
    }
    Ok(())
}

/// 退出竖屏：还原进入前的窗口几何与最小尺寸约束。
#[tauri::command]
pub fn exit_portrait_screen(app: AppHandle) -> AppResult<()> {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    // 退出前先把当前竖屏几何精确写进竖屏槽位（此刻窗态还是竖屏，save 按
    // 形态路由），理由同 exit_mini_screen
    crate::window_state::save(&win);
    let _ = portrait_full_restore().take();
    let saved = portrait_restore().take();
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
