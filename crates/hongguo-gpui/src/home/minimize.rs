//! 最小化自动暂停：窗口最小化 → 自动暂停；还原 → 补上欠的那次播放。判定走
//! 平台层的最小化态轮询（gpui 没有最小化事件面，间隔 300ms）。欠账模型：
//! 最小化瞬间本来在播才记欠，还原补上；用户自己按停的还原后仍停着。
//! `pauseOnMinimize` 开关每拍现读，改了即时生效。

use std::time::Duration;

use gpui_kit::prelude::*;

/// 轮询间隔：最小化到声音停止的感知上限。
const POLL_MS: u64 = 300;

impl super::HomeView {
    /// 起最小化监听（随视图生命周期常驻，设置开关每拍现读，关着时一拍
    /// 只是一次平台的最小化态查询）。
    pub(crate) fn spawn_minimize_poller(
        &mut self,
        handle: gpui_kit::AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(POLL_MS))
                    .await;
                // 最小化态要在窗口上下文里查（拿得到 Window）
                let Ok(minimized) =
                    handle.update(cx, |_, window, _| crate::platform::is_minimized(window))
                else {
                    return; // 窗口已关
                };
                let ok = this
                    .update(cx, |view, cx| view.minimize_tick(minimized, cx))
                    .is_ok();
                if !ok {
                    return; // 视图已销毁
                }
            }
        })
        .detach();
    }

    /// 一拍判定（主线程）。返回 `false` = 视图已销毁。
    fn minimize_tick(&mut self, minimized: bool, cx: &mut Context<Self>) -> bool {
        if minimized {
            if !self.state.settings().pause_on_minimize {
                return true;
            }
            if let Some(player) = &self.player
                && !player.is_paused()
            {
                // 只在「正在播」时欠：已经暂停着说明是用户自己按停的
                self.minimize_owed = true;
                player.pause();
                // 暂停 = 不再是「用户级工作」：活动断言一起收（用户手动暂停
                // 走 toggle_pause 也收，这里是同一套语义）。
                crate::play_activity::set_active(false);
                log::info!("[Home] 窗口最小化，自动暂停");
                cx.notify();
            }
        } else if self.minimize_owed {
            self.minimize_owed = false;
            if let Some(player) = &self.player {
                player.resume();
                // 恢复播放重新持有活动断言（同 toggle_pause）
                crate::play_activity::set_active(true);
                log::info!("[Home] 窗口还原，续播");
                cx.notify();
            }
        }
        true
    }
}
