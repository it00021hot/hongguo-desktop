//! 隐身模式（Tauri `incognito_cmd.rs` 同语义移植）：鼠标脱离窗口 → 暂停 +
//! 整窗藏起；回到窗口矩形 → 窗口原地重现 + 补上欠的那次播放。
//!
//! 隐藏的窗口收不到任何鼠标事件，「回来了」只能靠**系统级光标位置轮询**
//! 判定（与 Tauri 同参 120ms）——这是桌面端做这件事的唯一路径。轮询在
//! 壳内跑成一条 GPUI 任务（后台执行器定时，判定与藏显进主线程 update）；
//! 藏显走 [`crate::win32`]（gpui 的 `PlatformWindow` 只有只读的
//! visibility 面，按窗口藏显没有 API，见模块文档）。
//!
//! 欠账模型（与 Tauri 前端 owedPlay 同构）：隐藏瞬间本来在播才记「欠一次
//! 播放」，回来补上；用户自己按停的（隐藏前就是暂停态）回来仍停着。偏好
//! 经 [`crate::playback_prefs`] 跨启动保留（Tauri 存 localStorage 同语义）。
//! macOS：gpui 侧无按窗口藏显实现，先降级为不支持（与 mini 的 Apple 侧
//! 缺口同口径），后续补 FFI。

/// 一拍判定得出的窗口动作。**必须在实体更新之外执行**：ShowWindow 会同步
/// 派发 WM_SHOWWINDOW/WM_ACTIVATE，gpui 的窗口过程要更新窗口状态，在 update
/// 里调用会撞借用（实测日志刷「RefCell already borrowed」六连）。
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IncognitoStep {
    /// 鼠标离开：窗口藏起（播放已在判定里暂停）。
    Hide,
    /// 鼠标回来：窗口原地重现、不夺焦点（播放已在判定里补上）。
    ShowNoActivate,
}

#[cfg(target_os = "windows")]
use std::time::Duration;

use gpui_kit::Window;
use gpui_kit::prelude::*;

/// 轮询间隔：再快肉眼无感，再慢「回来」会迟半拍（Tauri INCOGNITO_POLL_MS）。
#[cfg(target_os = "windows")]
const POLL_MS: u64 = 120;

impl super::HomeView {
    /// 老板键：窗口被隐藏/恢复时的播放处置（Tauri 前端 `boss-key-hidden`
    /// 事件同款）——隐藏即暂停（声音不漏），恢复时把暂停着的续上。
    /// 窗口藏显由调用方（壳）做，这里只管播放。
    #[cfg_attr(
        not(target_os = "windows"),
        expect(unused_variables, reason = "老板键只有 Windows 侧有注册实现")
    )]
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
    /// `window` 只在 Windows 的句柄探测里用；mac 降级路径用不上。
    #[cfg_attr(
        not(target_os = "windows"),
        expect(unused_variables, reason = "mac 侧无窗口藏显 FFI，句柄探测分支未编译")
    )]
    pub(super) fn set_incognito(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.incognito == on {
            return;
        }
        self.incognito = on;
        if on {
            #[cfg(target_os = "windows")]
            match crate::win32::hwnd_of(window) {
                Some(hwnd) => self.spawn_incognito_poller(hwnd, cx),
                // 预览/无头环境拿不到原生句柄：开关翻回去，别留在半开态
                None => {
                    log::warn!("[Home] 隐身开启失败：拿不到原生窗口句柄");
                    self.incognito = false;
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                log::warn!("[Home] 隐身模式暂未在本平台实现（窗口藏显 FFI 后补）");
                self.incognito = false;
            }
        } else {
            if self.incognito_owed {
                self.incognito_owed = false;
                if let Some(player) = &self.player {
                    player.resume();
                }
            }
            #[cfg(target_os = "windows")]
            if let Some(hwnd) = crate::win32::hwnd_of(window) {
                // 轮询在下一拍自杀（incognito 已翻 false）；窗口这边先带回来，
                // 「用户亲手关隐身」就是要窗口，夺焦点合理
                crate::win32::show_foreground(hwnd);
            }
        }
        self.persist_prefs();
        cx.notify();
    }

    /// 起轮询任务（幂等：轮询已在跑就不重复起）。
    #[cfg(target_os = "windows")]
    pub(super) fn spawn_incognito_poller(&mut self, hwnd: isize, cx: &mut Context<Self>) {
        if self.incognito_polling {
            return;
        }
        self.incognito_polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(POLL_MS))
                    .await;
                let Some((keep_going, step)) = this
                    .update(cx, |view, cx| view.incognito_tick(hwnd, cx))
                    .ok()
                else {
                    return; // 视图已死
                };
                if !keep_going {
                    return;
                }
                match step {
                    Some(IncognitoStep::Hide) => crate::win32::hide(hwnd),
                    Some(IncognitoStep::ShowNoActivate) => crate::win32::show_no_activate(hwnd),
                    None => {}
                }
            }
        })
        .detach();
    }

    /// 一拍判定（主线程）：光标在窗口矩形内 == 窗口可见态 → 无事发生；
    /// 不一致才动。返回 `(是否继续轮询, 需要调用方执行的窗口动作)`；播放的
    /// 暂停/续播在这拍里就地做掉，窗口藏显交回调用方（见 [`IncognitoStep`]）。
    #[cfg(target_os = "windows")]
    fn incognito_tick(
        &mut self,
        hwnd: isize,
        cx: &mut Context<Self>,
    ) -> (bool, Option<IncognitoStep>) {
        if !self.incognito {
            self.incognito_polling = false;
            return (false, None);
        }
        // 最小化是用户的显式动作，隐身不得插手：最小化后光标多半还留在原
        // 窗口矩形里，不跳过会每拍都判成「回来了」把窗口 show 回来
        //（Tauri 2026-10-07 实测「隐身开着就最小化不了」）
        if crate::win32::is_iconic(hwnd) {
            return (true, None);
        }
        let Some((cx_pos, cy_pos)) = crate::win32::cursor_pos() else {
            return (true, None); // 光标位置拿不到：本轮不动，绝不基于未知状态藏窗
        };
        let Some((x, y, w, h)) = crate::win32::window_rect(hwnd) else {
            return (true, None);
        };
        let inside = cx_pos >= x && cy_pos >= y && cx_pos < x + w && cy_pos < y + h;
        let visible = crate::win32::is_window_visible(hwnd);
        if inside == visible {
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
