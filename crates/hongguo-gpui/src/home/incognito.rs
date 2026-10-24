//! 隐身模式（Tauri `incognito_cmd.rs` 同语义移植）：鼠标脱离窗口 → 暂停 +
//! 整窗藏起；回到窗口矩形 → 窗口原地重现 + 补上欠的那次播放。
//!
//! 隐藏的窗口收不到任何鼠标事件，「回来了」只能靠**系统级光标位置轮询**
//! 判定（与 Tauri 同参 120ms）——这是桌面端做这件事的唯一路径。轮询在
//! 壳内跑成一条 GPUI 任务（后台执行器定时，判定与藏显进主线程 update）。
//! 窗口藏显/光标位置全在 [`crate::platform`]，本模块零平台分支。
//!
//! 欠账模型（与 Tauri 前端 owedPlay 同构）：隐藏瞬间本来在播才记「欠一次
//! 播放」，回来补上；用户自己按停的（隐藏前就是暂停态）回来仍停着。偏好
//! 经 [`crate::playback_prefs`] 跨启动保留（Tauri 存 localStorage 同语义）。

use std::time::Duration;

use gpui_kit::Window;
use gpui_kit::prelude::*;

/// 一拍判定得出的窗口动作。**藏显由平台层延后到执行器下一拍执行**：
/// Windows 上 ShowWindow 会同步派发 WM_SHOWWINDOW/WM_ACTIVATE、macOS 的
/// orderOut/orderFront 会同步投递窗口通知，gpui 的窗口过程/通知回调都要在
/// 这时更新窗口状态，在借用内调用会撞借用（gpui 用 try_borrow_mut 拒绝那次
/// 更新，把错吞成 "RefCell already borrowed" 的空 target 日志）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IncognitoStep {
    /// 鼠标离开：窗口藏起（播放已在判定里暂停）。
    Hide,
    /// 鼠标回来：窗口原地重现、不夺焦点（播放已在判定里补上）。
    ShowNoActivate,
}

/// 轮询间隔：再快肉眼无感，再慢「回来」会迟半拍（Tauri INCOGNITO_POLL_MS）。
const POLL_MS: u64 = 120;

/// 一拍所需的环境读数（主线程采集，判定本身是纯逻辑）。
struct IncognitoProbe {
    cursor: Option<(f64, f64)>,
    /// (x, y, w, h)，与 `cursor` 同一坐标空间。
    rect: Option<(f64, f64, f64, f64)>,
    visible: bool,
    minimized: bool,
}

/// 采集窗口读数（坐标空间由平台实现保证光标与窗口一致）。
fn probe(window: &Window) -> IncognitoProbe {
    IncognitoProbe {
        cursor: crate::platform::cursor_pos(window),
        rect: crate::platform::window_rect(window),
        visible: crate::platform::is_visible(window),
        minimized: crate::platform::is_minimized(window),
    }
}

/// 执行窗口动作（实体更新之外调用）。
///
/// 平台层自己把它延后到执行器下一拍：Windows 的 ShowWindow 同步派发
/// WM_SHOWWINDOW/WM_ACTIVATE、macOS 的 orderOut/orderFront 同步投递窗口
/// 通知，gpui 的窗口过程/通知回调都要在这时更新窗口状态——在 update 借用内
/// 调用会撞借用（gpui 用 try_borrow_mut 拒绝那次更新，把错吞成
/// "RefCell already borrowed" 的空 target 日志）。
fn apply_step(window: &Window, cx: &gpui_kit::App, step: IncognitoStep) {
    match step {
        IncognitoStep::Hide => crate::platform::hide(window, cx),
        IncognitoStep::ShowNoActivate => crate::platform::show_no_activate(window, cx),
    }
}

impl super::HomeView {
    /// 老板键：窗口被隐藏/恢复时的播放处置（Tauri 前端 `boss-key-hidden`
    /// 事件同款）——隐藏即暂停（声音不漏），恢复时把暂停着的续上。
    /// 窗口藏显由调用方（壳）做，这里只管播放。
    pub(crate) fn boss_key_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let Some(player) = &self.player else {
            return;
        };
        if hidden {
            player.pause();
        } else if player.is_paused() {
            player.resume();
        }
        cx.notify();
    }

    /// 开关隐身模式。开 = 起光标轮询（盯着本窗口）；关 = 停轮询，立刻把
    /// 可能正处在隐藏态的窗口带回前台并补上欠账（Tauri set_incognito(false)
    /// 同款——它对「挂载同步」的防重入在这里不存在：本壳只有这一个开关入口，
    /// 关的时候窗口一定在恢复路径上）。
    pub(super) fn set_incognito(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.incognito == on {
            return;
        }
        self.incognito = on;
        if on {
            self.spawn_incognito_poller(window.window_handle(), cx);
        } else {
            if self.incognito_owed {
                self.incognito_owed = false;
                if let Some(player) = &self.player {
                    player.resume();
                }
            }
            let handle = window.window_handle();
            // 轮询在下一拍自杀（incognito 已翻 false）；窗口这边先带回来，
            // 「用户亲手关隐身」就是要窗口，夺焦点合理
            let _ = handle.update(cx, |_, window, cx| {
                crate::platform::show_foreground(window, cx)
            });
        }
        self.persist_prefs();
        cx.notify();
    }

    /// 起轮询任务（幂等：轮询已在跑就不重复起）。
    pub(super) fn spawn_incognito_poller(
        &mut self,
        handle: gpui_kit::AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        if self.incognito_polling {
            return;
        }
        self.incognito_polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(POLL_MS))
                    .await;
                // 读数要在窗口上下文里采（拿得到 Window），判定与实体写入
                // 在 update 里，窗口动作再回窗口上下文执行——三段分开，
                // 沿用「窗口操作不嵌在实体更新里」的纪律
                let Ok(read) = handle.update(cx, |_, window, _| probe(window)) else {
                    return; // 窗口已关
                };
                let Ok((keep_going, step)) =
                    this.update(cx, |view, cx| view.incognito_tick(&read, cx))
                else {
                    return; // 视图已死
                };
                if !keep_going {
                    return;
                }
                if let Some(step) = step {
                    let _ = handle.update(cx, |_, window, cx| apply_step(window, cx, step));
                }
            }
        })
        .detach();
    }

    /// 一拍判定（主线程）：光标在窗口矩形内 == 窗口可见态 → 无事发生；
    /// 不一致才动。返回 `(是否继续轮询, 需要调用方执行的窗口动作)`；播放的
    /// 暂停/续播在这拍里就地做掉，窗口藏显交回调用方（见 [`IncognitoStep`]）。
    fn incognito_tick(
        &mut self,
        read: &IncognitoProbe,
        cx: &mut Context<Self>,
    ) -> (bool, Option<IncognitoStep>) {
        if !self.incognito {
            self.incognito_polling = false;
            return (false, None);
        }
        // 最小化是用户的显式动作，隐身不得插手：最小化后光标多半还留在原
        // 窗口矩形里，不跳过会每拍都判成「回来了」把窗口 show 回来
        //（Tauri 2026-10-07 实测「隐身开着就最小化不了」）
        if read.minimized {
            return (true, None);
        }
        let (Some((cx_pos, cy_pos)), Some((x, y, w, h))) = (read.cursor, read.rect) else {
            return (true, None); // 读数拿不到：本轮不动，绝不基于未知状态藏窗
        };
        let inside = cx_pos >= x && cy_pos >= y && cx_pos < x + w && cy_pos < y + h;
        if inside == read.visible {
            return (true, None);
        }
        let step = if inside {
            // 鼠标回来了：窗口原地重现（不夺焦点就不打断用户手上的事），
            // 补上欠的那次播放
            if self.incognito_owed {
                self.incognito_owed = false;
                if let Some(player) = &self.player {
                    player.resume();
                }
            }
            IncognitoStep::ShowNoActivate
        } else {
            // 鼠标走了：先暂停再藏。暂停同步落在引擎上（音频设备停消费），
            // 不需要 Tauri 的 80ms 送达窗——那是给 IPC 通知前端的余量
            if let Some(player) = &self.player {
                self.incognito_owed = !player.is_paused();
                if self.incognito_owed {
                    player.pause();
                }
            }
            IncognitoStep::Hide
        };
        cx.notify();
        (true, Some(step))
    }
}
