//! MediaEngine 事件回调 → [`PlayerEvent`]。
//!
//! 引擎在自己的线程上调 `EventNotify`，这里只做节流去重 + 送通道（`blocking_send`
//! 允许在同步线程上用）。事件语义与 macOS 侧的事件线程一致：
//! 首帧 / 缓冲开始 / 缓冲结束 / 播完 / 失败，各一次。

use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_playback::PlayerEvent;
use tokio::sync::mpsc;
use windows::Win32::Media::MediaFoundation::{
    IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_EVENT_BUFFERINGENDED,
    MF_MEDIA_ENGINE_EVENT_BUFFERINGSTARTED, MF_MEDIA_ENGINE_EVENT_CANPLAYTHROUGH,
    MF_MEDIA_ENGINE_EVENT_ENDED, MF_MEDIA_ENGINE_EVENT_ERROR,
    MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY, MF_MEDIA_ENGINE_EVENT_PLAYING,
    MF_MEDIA_ENGINE_EVENT_STALLED, MF_MEDIA_ENGINE_EVENT_STREAMRENDERINGERROR,
    MF_MEDIA_ENGINE_EVENT_WAITING,
};
use windows::core::{Result, implement};

/// 一集的事件通道（引擎持有；通道关闭 = 宿主已弃，发送静默失败即可）。
#[implement(IMFMediaEngineNotify)]
pub(super) struct Notify {
    events: mpsc::Sender<PlayerEvent>,
    /// 首次出画只报一次（`FIRSTFRAMEREADY` 与随后的 `PLAYING` 都可能到）。
    started: AtomicBool,
    /// 缓冲状态机去重：连续同类事件只发一次。
    buffering: AtomicBool,
}

impl Notify {
    pub(super) fn new(events: mpsc::Sender<PlayerEvent>) -> Self {
        Self {
            events,
            started: AtomicBool::new(false),
            buffering: AtomicBool::new(false),
        }
    }

    fn send(&self, event: PlayerEvent) {
        // 宿主丢弃事件循环后通道关闭：静默丢弃，不 panic。
        let _ = self.events.blocking_send(event);
    }

    fn first_frame(&self) {
        if !self.started.swap(true, Ordering::SeqCst) {
            // MediaEngine 的事件不带尺寸；壳侧 system 分支只用它置「画面已就绪」，
            // 真实尺寸在 `PlayerLike::dims` 里另外查引擎。
            self.send(PlayerEvent::FirstFrame {
                width: 0,
                height: 0,
            });
            self.send(PlayerEvent::Ready);
        }
    }

    fn buffering(&self, active: bool) {
        if self.buffering.swap(active, Ordering::SeqCst) != active {
            self.send(if active {
                PlayerEvent::Buffering
            } else {
                PlayerEvent::Ready
            });
        }
    }
}

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, param2: u32) -> Result<()> {
        match event {
            value
                if value == MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY.0 as u32
                    || value == MF_MEDIA_ENGINE_EVENT_PLAYING.0 as u32 =>
            {
                self.first_frame();
                self.buffering(false);
            }
            value
                if value == MF_MEDIA_ENGINE_EVENT_BUFFERINGSTARTED.0 as u32
                    || value == MF_MEDIA_ENGINE_EVENT_WAITING.0 as u32
                    || value == MF_MEDIA_ENGINE_EVENT_STALLED.0 as u32 =>
            {
                self.buffering(true);
            }
            value
                if value == MF_MEDIA_ENGINE_EVENT_BUFFERINGENDED.0 as u32
                    || value == MF_MEDIA_ENGINE_EVENT_CANPLAYTHROUGH.0 as u32 =>
            {
                self.buffering(false);
            }
            value if value == MF_MEDIA_ENGINE_EVENT_ENDED.0 as u32 => {
                self.buffering(false);
                self.send(PlayerEvent::Ended);
            }
            value
                if value == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32
                    || value == MF_MEDIA_ENGINE_EVENT_STREAMRENDERINGERROR.0 as u32 =>
            {
                // param1 = MF_MEDIA_ENGINE_ERR 类别，param2 = 扩展 HRESULT。
                log::error!(
                    "[WmPresenter] MediaEngine 报错：类型 {param1}，HRESULT 0x{param2:08X}"
                );
                self.send(PlayerEvent::Failed(format!(
                    "MediaEngine 播放失败（类型 {param1}，HRESULT 0x{param2:08X}）"
                )));
            }
            _ => {}
        }
        Ok(())
    }
}
