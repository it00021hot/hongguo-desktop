//! 壳内唯一的平台代码（`windows.rs` / `macos.rs` 各一份实现）：单实例、
//! 窗口几何（三窗态 + 竖屏铺满 + 全屏）、无边框、隐身透明、置顶、光标。
//!
//! 为什么还留在壳里：nsh 0.1.1 的 Rust 面**没有**窗口几何 API——`AppView`
//! 只交出 `hwnd()` / `view()`，而三壳的小窗/竖屏语义（对齐 Tauri
//! `window_cmd.rs`：同一个窗口原位变形、播放零中断）只能在原生层做一次。
//! 窗态几何的**跨启动记忆**在 [`crate::window_state`]（三槽位 JSON）。
//!
//! 分层纪律：业务/视图层只调本模块的门面，`#[cfg]` 平台分支不出
//! `platform/`；几何一律**逻辑点**、窗口外框语义（恢复时
//! `apply_content_and_origin` 内部按 dpi 换算）。
//!
//! 快照纪律（对齐 Tauri `window_cmd.rs`）：进形态前先把当前几何**精确写
//! 进常态槽位**再快照（防抖线程 500ms 一拍，紧接着关窗会把最后一次拖动
//! 落在档外）；退出前把当前形态几何写回自己的槽位；铺满档另存一份普通档
//! 几何。恢复校验：尺寸下限 + 与显示器工作区相交。

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "macos")]
pub(crate) use macos as imp;
#[cfg(target_os = "windows")]
pub(crate) use windows as imp;

use crate::window_state::{self, Geometry, Mode};

use std::sync::atomic::{AtomicBool, Ordering};

/// 小屏尺寸与落角边距（与 Tauri 壳 `window_cmd.rs` 同参）。
pub(crate) const MINI_W: f32 = 480.0;
pub(crate) const MINI_H: f32 = 270.0;
pub(crate) const MINI_MARGIN: f32 = 16.0;
/// 小屏下的最小尺寸：控制条一行固定内容约 352px，360 是「不换行」的下限。
pub(crate) const MINI_MIN: (f32, f32) = (360.0, 180.0);
/// 竖屏下的最小尺寸（window_cmd 的 PORTRAIT_MIN）。
pub(crate) const PORTRAIT_MIN: (f32, f32) = (360.0, 400.0);
/// 常态最小尺寸（＝小屏同款：常态也允许缩到迷你尺寸，#5）。
pub(crate) const MAIN_MIN: (f32, f32) = (MINI_W, MINI_H);
/// 普通档竖屏的高度上限 = 工作区高 × 此系数（window_cmd 刻意差异第三处）。
const PORTRAIT_HEIGHT_RATIO: f32 = 0.7;

/// 进小屏/竖屏前要还原的常态几何（互相不叠加：小屏与竖屏入口互斥，
/// 前端守卫保证）。
static RESTORE_NORMAL: std::sync::Mutex<Option<Geometry>> = std::sync::Mutex::new(None);
/// 铺满档退出时要还原的普通档几何。
static RESTORE_PORTRAIT_FULL: std::sync::Mutex<Option<Geometry>> = std::sync::Mutex::new(None);
/// 当前窗态（几何落盘路由用；竖屏优先于小屏，与 Tauri `current_mode` 同款）。
static CURRENT_MODE: std::sync::Mutex<Mode> = std::sync::Mutex::new(Mode::Normal);
/// 铺满档镜像态（窗口几何不区分它，播放域的 refit/图标要用）。
static PORTRAIT_FULL: AtomicBool = AtomicBool::new(false);

fn current_mode() -> Mode {
    *CURRENT_MODE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_current_mode(mode: Mode) {
    *CURRENT_MODE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = mode;
}

// ---------------------------------------------------------------- 单实例

pub fn claim_single_instance() -> bool {
    imp::claim_single_instance()
}

pub fn take_focus_request() -> bool {
    imp::take_focus_request()
}

pub fn bring_to_front() {
    imp::bring_to_front()
}

// ---------------------------------------------------------------- 光标

pub fn cursor_in_window() -> Option<(f32, f32)> {
    imp::cursor_in_window()
}

// ---------------------------------------------------------------- 形态切换

/// 当前几何（外框，逻辑点）。
fn current_geometry() -> Option<Geometry> {
    imp::window_rect()
}

/// 几何记忆是否可用：尺寸不小于该形态下限，且与显示器工作区相交
/// （Tauri `load_valid` 同口径——拔掉显示器的旧坐标会把窗口丢到屏外）。
fn slot_valid(g: &Geometry, min: (f32, f32)) -> bool {
    g.width >= min.0 && g.height >= min.1 && imp::on_any_monitor(g)
}

/// 进入小屏播放（对齐 `enter_mini_screen`）：幂等；先把当前几何写进常态
/// 槽位再快照；小屏槽位命中就原样回去，否则 480×270 落工作区右下角。
pub fn enter_mini() {
    if current_mode() == Mode::Mini {
        return;
    }
    if let Some(g) = current_geometry() {
        window_state::store(Mode::Normal, g);
        *RESTORE_NORMAL.lock().unwrap_or_else(|p| p.into_inner()) = Some(g);
    }
    set_current_mode(Mode::Mini);
    let remembered = window_state::slot(Mode::Mini).filter(|g| slot_valid(g, MINI_MIN));
    match remembered {
        Some(g) => apply_outer(g),
        None => {
            let (left, top, right, bottom) = imp::work_area();
            // 小工作区放不下就压到放得下为止；手动 min/max 而非 clamp
            // （极小工作区下 min>max 时 clamp 会 panic）。
            let w = MINI_W.min(right - left - 2.0 * MINI_MARGIN).max(200.0);
            let h = MINI_H.min(bottom - top - 2.0 * MINI_MARGIN).max(150.0);
            let x = (right - w - MINI_MARGIN).max(left);
            let y = (bottom - h - MINI_MARGIN).max(top);
            imp::apply_content_and_origin(w, h, x, y);
        }
    }
    log::info!("[Shell] 窗口形态 → Mini");
}

/// 退出小屏：小屏几何先写回小屏槽位，再还原进入前几何。
pub fn exit_mini() {
    if current_mode() != Mode::Mini {
        return;
    }
    if let Some(g) = current_geometry() {
        window_state::store(Mode::Mini, g);
    }
    set_current_mode(Mode::Normal);
    let saved = RESTORE_NORMAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let Some(g) = saved {
        apply_outer(g);
    }
    log::info!("[Shell] 窗口形态 → Normal（离开小屏）");
}

/// 按视频宽高比重算竖屏几何并落窗（window_cmd `fit_portrait` 逐步同款：
/// 普通档高保持当前高但最多工作区 70%；铺满档撑满工作区；宽溢出反推高；
/// x 普通档在当前窗口上居中、铺满档居中于工作区）。
pub fn fit_portrait(aspect: f32, full: bool) {
    let Some(cur) = imp::window_rect() else {
        return;
    };
    let (left, top, right, bottom) = imp::work_area();
    let (wa_w, wa_h) = (right - left, bottom - top);
    let mut h = if full {
        wa_h
    } else {
        cur.height.min((wa_h * PORTRAIT_HEIGHT_RATIO).round())
    };
    let mut w = (h * aspect).round();
    if w > wa_w {
        w = wa_w;
        h = (w / aspect).round();
    }
    w = w.max(PORTRAIT_MIN.0);
    let x = if full {
        left + ((wa_w - w) / 2.0).round().max(0.0)
    } else {
        // 手动 min/max 而非 clamp（极小工作区下 min>max 时 clamp 会 panic）。
        (cur.x + ((cur.width - w) / 2.0).round())
            .max(left)
            .min(right - w)
    };
    let y = if full { top } else { cur.y };
    imp::apply_content_and_origin(w, h, x, y);
}

/// 进入竖屏播放（对齐 `enter_portrait_screen`）：幂等 + 宽高比守卫；
/// 竖屏槽位命中就原样回去，否则按普通档现算。
pub fn enter_portrait(aspect: f32) {
    if current_mode() == Mode::Portrait || !aspect.is_finite() || aspect <= 0.0 {
        return;
    }
    if let Some(g) = current_geometry() {
        window_state::store(Mode::Normal, g);
        *RESTORE_NORMAL.lock().unwrap_or_else(|p| p.into_inner()) = Some(g);
    }
    set_current_mode(Mode::Portrait);
    PORTRAIT_FULL.store(false, Ordering::Release);
    let remembered = window_state::slot(Mode::Portrait).filter(|g| slot_valid(g, PORTRAIT_MIN));
    match remembered {
        Some(g) => apply_outer(g),
        None => fit_portrait(aspect, false),
    }
    log::info!("[Shell] 窗口形态 → Portrait（aspect={aspect:.3}）");
}

/// 铺满档切换（对齐 `set_portrait_full`）：普通→铺满先快照再撑满；
/// 铺满→普通还原快照（没有快照就按普通档重算）。不在竖屏中则忽略。
pub fn set_portrait_full(aspect: f32, full: bool) {
    if current_mode() != Mode::Portrait {
        return;
    }
    PORTRAIT_FULL.store(full, Ordering::Release);
    if full {
        if let Some(g) = current_geometry() {
            *RESTORE_PORTRAIT_FULL
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = Some(g);
        }
        fit_portrait(aspect, true);
    } else {
        let saved = RESTORE_PORTRAIT_FULL
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        match saved {
            Some(g) => apply_outer(g),
            None => fit_portrait(aspect, false),
        }
    }
}

/// 竖屏中的铺满镜像态（播放域的图标态与 refit 档位用）。
#[expect(dead_code, reason = "窗口侧真值；播放域用镜像信号，升级后统一")]
pub fn portrait_full() -> bool {
    PORTRAIT_FULL.load(Ordering::Acquire)
}

/// 退出竖屏：竖屏几何先写回竖屏槽位，再还原进入前几何并清铺满镜像。
pub fn exit_portrait() {
    if current_mode() != Mode::Portrait {
        return;
    }
    if let Some(g) = current_geometry() {
        window_state::store(Mode::Portrait, g);
    }
    PORTRAIT_FULL.store(false, Ordering::Release);
    set_current_mode(Mode::Normal);
    RESTORE_PORTRAIT_FULL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    let saved = RESTORE_NORMAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let Some(g) = saved {
        apply_outer(g);
    }
    log::info!("[Shell] 窗口形态 → Normal（离开竖屏）");
}

/// 把**外框**几何原样落回窗口。
fn apply_outer(g: Geometry) {
    // apply_content_and_origin 的尺寸语义是内容尺寸；外框≈内容（本壳
    // 无边框窗口标题栏为 0，误差在一帧内自愈）。
    imp::apply_content_and_origin(g.width, g.height, g.x, g.y);
}

// ---------------------------------------------------------------- 全屏/无边框/隐身

/// 全屏开关（播放控制栏按钮；窗口操作在主线程回调内）。
pub fn toggle_fullscreen() {
    imp::toggle_fullscreen();
}

/// 是否全屏。
pub fn is_fullscreen() -> bool {
    imp::is_fullscreen()
}

/// 无边框播放开关（App chrome 藏显由 `app.rs` 按信号裁决；窗口侧 macOS
/// 藏红绿灯，Windows 无边框本就无边框）。
pub fn set_frameless(on: bool) {
    imp::set_frameless(on);
}

/// 窗口透明度（0..1，隐身模式用）。
pub fn set_window_alpha(alpha: f32) {
    imp::set_window_alpha(alpha);
}

/// 置顶开关（窗口级会话态：进小屏/竖屏不清，Tauri 同款）。
pub fn set_always_on_top(top: bool) {
    imp::set_always_on_top(top)
}

// ---------------------------------------------------------------- 几何记忆

/// 启动恢复常态几何：窗口永远以常态开（形态是会话态）。校验通过就落窗，
/// 否则保持 `AppOptions.size` 的默认。
pub fn restore_normal() {
    let Some(g) = window_state::slot(Mode::Normal).filter(|g| slot_valid(g, MAIN_MIN)) else {
        log::info!("[Window] 无常态几何记忆（或不在屏内），按默认尺寸");
        return;
    };
    apply_outer(g);
    log::info!(
        "[Window] 已恢复常态几何（{:.0}×{:.0} @ {:.0},{:.0}）",
        g.width,
        g.height,
        g.x,
        g.y
    );
}

/// 几何防抖落盘（对齐 Tauri `spawn_saver`）：500ms 一拍，几何变了才写；
/// 全屏中不落（尺寸归系统管）。跑在独立线程（Win32 查询不限线程；
/// macOS 的窗口读取需要主线程——拿不到 marker 就跳过这一拍）。
pub fn spawn_geometry_saver() {
    std::thread::Builder::new()
        .name("hongguo-nsh-geometry".into())
        .spawn(move || {
            let mut last: Option<Geometry> = None;
            // 窗口由 run_app 创建：见到过窗口之后的 None 才是「关窗退出」，
            // 启动竞速期的 None 只是窗口还没起来，继续等。
            let mut seen_window = false;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if imp::is_fullscreen() {
                    continue;
                }
                let Some(g) = current_geometry() else {
                    if seen_window {
                        // 窗口已关（进程退出中）：把上一帧几何补写进当前槽位。
                        if let Some(g) = last {
                            window_state::store(current_mode(), g);
                        }
                        return;
                    }
                    continue;
                };
                seen_window = true;
                if last.is_some_and(|prev| prev == g) {
                    continue;
                }
                let changed_from_last = last.replace(g).is_some();
                // 首帧只记账不落盘（启动恢复那一下不该立刻覆盖档面）。
                if changed_from_last {
                    window_state::store(current_mode(), g);
                }
            }
        })
        .expect("几何落盘线程创建失败");
}
