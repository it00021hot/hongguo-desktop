//! 内置视频解码器使用的音频播放器与媒体时钟。

use super::{avf, loader};
use hongguo_player_api::PlayerEvent;
use objc2::{rc::Retained, runtime::AnyObject};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const EVENT_TICK: Duration = Duration::from_millis(100);
pub(crate) const FORWARD_BUFFER_SECS: f64 = 2.0;
pub(crate) struct SendRetained(pub(crate) Retained<AnyObject>);
// SAFETY: AVPlayer controls and immutable retained objects support cross-thread access.
unsafe impl Send for SendRetained {}
unsafe impl Sync for SendRetained {}
impl SendRetained {
    pub(crate) fn get(&self) -> &AnyObject {
        &self.0
    }
    pub(crate) fn clone_ref(&self) -> Self {
        Self(self.0.clone())
    }
}

pub(crate) struct AudioPlayer {
    pub(crate) player: SendRetained,
    pub(crate) current_loader: Mutex<Option<loader::LoaderHandle>>,
    pub(crate) current_asset: Mutex<Option<SendRetained>>,
    stop: Arc<AtomicBool>,
    event_thread: Mutex<Option<JoinHandle<()>>>,
    pub(crate) paused: AtomicBool,
    rate: AtomicU64,
}

impl AudioPlayer {
    pub(crate) fn assemble(player: Retained<AnyObject>, rate: f64) -> Arc<Self> {
        Arc::new(Self {
            player: SendRetained(player),
            current_loader: Mutex::new(None),
            current_asset: Mutex::new(None),
            stop: Arc::new(AtomicBool::new(false)),
            event_thread: Mutex::new(None),
            paused: AtomicBool::new(false),
            rate: AtomicU64::new(rate.to_bits()),
        })
    }

    pub(crate) fn spawn_events(&self, events: mpsc::Sender<PlayerEvent>) {
        let player = self.player.clone_ref();
        let stop = self.stop.clone();
        let thread = std::thread::Builder::new()
            .name("hongguo-audio-events".into())
            .spawn(move || {
                let player = player.get();
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(item) = avf::current_item(player) {
                        match avf::item_status(item) {
                            1 => break,
                            2 => {
                                let message = avf::item_error(item)
                                    .unwrap_or_else(|| "AVPlayerItem 加载失败".into());
                                let _ = events.blocking_send(PlayerEvent::Failed(message));
                                return;
                            }
                            _ => {}
                        }
                    }
                    if Instant::now() >= deadline {
                        let _ = events.blocking_send(PlayerEvent::Failed(
                            "AVPlayerItem 未就绪（15s 超时）".into(),
                        ));
                        return;
                    }
                    std::thread::sleep(EVENT_TICK);
                }
                let mut buffering = false;
                let mut ended = false;
                loop {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Some(item) = avf::current_item(player) else {
                        std::thread::sleep(EVENT_TICK);
                        continue;
                    };
                    if avf::item_status(item) == 2 {
                        let message =
                            avf::item_error(item).unwrap_or_else(|| "AVPlayerItem 播放失败".into());
                        let _ = events.blocking_send(PlayerEvent::Failed(message));
                        return;
                    }
                    let keep_up = avf::item_likely_to_keep_up(item);
                    if !keep_up && !buffering {
                        buffering = true;
                        let _ = events.blocking_send(PlayerEvent::Buffering);
                    } else if keep_up && buffering {
                        buffering = false;
                        let _ = events.blocking_send(PlayerEvent::Ready);
                    }
                    let duration = avf::item_duration(item).seconds();
                    let position = avf::current_time(player).seconds();
                    if !ended
                        && duration > 0.0
                        && position >= duration - 0.25
                        && avf::player_rate(player) > 0.0
                    {
                        ended = true;
                        let _ = events.blocking_send(PlayerEvent::Ended);
                    }
                    std::thread::sleep(EVENT_TICK);
                }
            })
            .expect("音频事件线程创建失败");
        *self.event_thread.lock().expect("事件线程锁 poisoned") = Some(thread);
    }
    pub(crate) fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        avf::pause(self.player.get());
    }
    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        avf::play_immediately(self.player.get(), self.rate() as f32);
    }
    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    pub(crate) fn position_secs(&self) -> f64 {
        avf::current_time(self.player.get()).seconds()
    }
    pub(crate) fn duration_secs(&self) -> f64 {
        avf::current_item(self.player.get())
            .map(|item| avf::item_duration(item).seconds())
            .unwrap_or(0.0)
    }
    pub(crate) fn seek(&self, seconds: f64) {
        avf::seek_exact(self.player.get(), avf::CmTime::secs(seconds.max(0.0)));
    }
    pub(crate) fn set_rate(&self, rate: f64) {
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
    }
    pub(crate) fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Relaxed))
    }
    pub(crate) fn set_volume(&self, volume: f32) {
        avf::set_volume(self.player.get(), volume.clamp(0.0, 1.0));
    }
    pub(crate) fn set_muted(&self, muted: bool) {
        avf::set_muted(self.player.get(), muted);
    }
}
impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        avf::pause(self.player.get());
        if let Some(asset) = self
            .current_asset
            .lock()
            .expect("当前 asset 锁 poisoned")
            .take()
        {
            avf::clear_resource_loader_delegate(asset.get());
            avf::cancel_loading(asset.get());
        }
        *self.current_loader.lock().expect("当前 loader 锁 poisoned") = None;
        if let Some(thread) = self
            .event_thread
            .lock()
            .expect("事件线程锁 poisoned")
            .take()
        {
            let _ = thread.join();
        }
    }
}
