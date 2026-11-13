use crate::{bridge::Bridge, worker};
use hongguo_nsh_video_api::{Seek, VideoEvent, VideoProps};
use ndk::native_window::NativeWindow;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{SyncSender, sync_channel},
};

#[derive(Clone, Default)]
pub(crate) struct Viewport {
    pub window: Option<NativeWindow>,
    pub width: u32,
    pub height: u32,
    pub version: u64,
}
pub(crate) struct Mailbox {
    pub props: VideoProps,
    pub viewport: Viewport,
    pub size: Option<(u32, u32)>,
    pub seek: Option<Seek>,
}
pub(crate) struct Shared {
    pub mailbox: Mutex<Mailbox>,
    pub bridge: Bridge,
    pub generation: AtomicU64,
    pub revision: AtomicU64,
    pub stop: AtomicBool,
    pub frame: AtomicBool,
    pub detached: AtomicU64,
    pub direct: Mutex<Option<crate::direct::DirectSlot>>,
    pub audio_position: AtomicU64,
    pub audio_seeking: AtomicBool,
    wake: SyncSender<()>,
}
impl Shared {
    pub fn spawn(bridge: Bridge) -> Result<Arc<Self>, String> {
        let (wake, receiver) = sync_channel(1);
        let shared = Arc::new(Self {
            mailbox: Mutex::new(Mailbox {
                props: VideoProps::default(),
                viewport: Viewport::default(),
                size: None,
                seek: None,
            }),
            bridge,
            generation: AtomicU64::new(0),
            revision: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            frame: AtomicBool::new(false),
            detached: AtomicU64::new(u64::MAX),
            direct: Mutex::new(None),
            audio_position: AtomicU64::new(0),
            audio_seeking: AtomicBool::new(false),
            wake,
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("hongguo-video-android".into())
            .spawn(move || worker::run(worker, receiver))
            .map_err(|error| error.to_string())?;
        Ok(shared)
    }
    pub fn wake(&self) {
        let _ = self.wake.try_send(());
    }
    pub fn props(&self, props: VideoProps) -> Result<(), String> {
        if !props.rate.is_finite()
            || props.rate <= 0.0
            || !props.volume.is_finite()
            || !(0.0..=1.0).contains(&props.volume)
            || props.enhancement.strength > 100
        {
            return Err("invalid playback rate, volume or enhancement strength".into());
        }
        let mut mailbox = self.mailbox.lock().expect("Android video mailbox");
        if mailbox.props == props {
            return Ok(());
        }
        let old_direct = if mailbox.props.source != props.source {
            self.generation.fetch_add(1, Ordering::AcqRel);
            mailbox.size = None;
            mailbox.seek = None;
            self.frame.store(false, Ordering::Release);
            self.direct.lock().expect("Android direct source").take()
        } else {
            None
        };
        mailbox.props = props;
        self.revision.fetch_add(1, Ordering::Release);
        drop(mailbox);
        if let Some(direct) = old_direct {
            direct.lease.close();
        }
        self.wake();
        Ok(())
    }
    pub fn close_direct(&self) {
        let old = self.direct.lock().expect("Android direct source").take();
        if let Some(direct) = old {
            direct.lease.close();
        }
    }
    pub fn event(&self, generation: u64, event: VideoEvent) {
        let mut mailbox = self.mailbox.lock().expect("Android video mailbox");
        if self.stop.load(Ordering::Acquire)
            || generation != self.generation.load(Ordering::Acquire)
            || mailbox
                .props
                .source
                .as_ref()
                .is_none_or(|source| source.id != source_id(&event))
        {
            return;
        }
        if let VideoEvent::Load { width, height, .. } = &event
            && *width > 0
            && *height > 0
        {
            mailbox.size = Some((*width, *height));
        }
        drop(mailbox);
        if generation == self.generation.load(Ordering::Acquire) {
            self.bridge.event(&event);
        }
        self.wake();
    }
}
fn source_id(event: &VideoEvent) -> &str {
    match event {
        VideoEvent::LoadStart { source_id }
        | VideoEvent::Load { source_id, .. }
        | VideoEvent::Progress { source_id, .. }
        | VideoEvent::Buffer { source_id, .. }
        | VideoEvent::End { source_id }
        | VideoEvent::Seeked { source_id, .. }
        | VideoEvent::Error { source_id, .. }
        | VideoEvent::EnhancementStatus { source_id, .. } => source_id,
    }
}
