//! Process-local native frame sources. Registration owns the shared session;
//! closing a consumer only retires that consumer.
use hongguo_media_types::{DecodeDevice, PresentationFrame};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const SCHEME: &str = "hongguo-frames";
const MAX_TOKEN_BYTES: usize = 128;
const BGRA_BYTES_PER_PIXEL: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct PixelFrame {
    pub serial: u64,
    pub generation: u64,
    pub pts_ms: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// Immutable BGRA8, with at least stride * height bytes.
    pub pixels: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackControl {
    pub playing: bool,
    pub volume: f32,
    pub muted: bool,
    pub rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackPosition {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub ended: bool,
    pub buffering: bool,
    /// An explicit empty video edit: present no media while the clock advances.
    pub video_gap: bool,
}

pub trait FrameSource: Send + Sync {
    fn info(&self) -> Result<Option<FrameInfo>, String>;
    fn frame_at(&self, position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String>;
    fn seek(&self, position_ms: u64) -> Result<u64, String>;
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String>;
    fn holding(&self) -> bool;
    fn close(&self);
    fn native_frame_at(
        &self,
        _position_secs: f64,
    ) -> Result<Option<Arc<PresentationFrame>>, String> {
        Ok(None)
    }
    fn configure_decoder(&self, _device: Option<Arc<dyn DecodeDevice>>) -> Result<(), String> {
        Ok(())
    }
    fn control(&self, _control: PlaybackControl) -> Result<(), String> {
        Ok(())
    }
    fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        Ok(None)
    }
    fn reject_decoder(&self, detail: &str) -> Result<(), String> {
        Err(detail.to_owned())
    }
    fn recover_presentation(&self, detail: &str) -> Result<(), String> {
        Err(detail.to_owned())
    }
}

struct Registered {
    source: Arc<dyn FrameSource>,
    revoked: AtomicBool,
    control: Mutex<Option<(u64, PlaybackControl)>>,
}
type Registry = Mutex<HashMap<String, Weak<Registered>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::default)
}

fn lock_registry() -> std::sync::MutexGuard<'static, HashMap<String, Weak<Registered>>> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_TOKEN_BYTES
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub struct FrameRegistration {
    token: String,
    url: String,
    registered: Arc<Registered>,
}

impl FrameRegistration {
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for FrameRegistration {
    fn drop(&mut self) {
        self.registered.revoked.store(true, Ordering::Release);
        lock_registry().remove(&self.token);
        self.registered.source.close();
    }
}

pub fn register_frames(
    token: &str,
    source: Arc<dyn FrameSource>,
) -> Result<FrameRegistration, String> {
    if !valid_token(token) {
        return Err("invalid frame source token".into());
    }
    let registered = Arc::new(Registered {
        source,
        revoked: AtomicBool::new(false),
        control: Mutex::new(None),
    });
    let mut registry = lock_registry();
    if registry.get(token).and_then(Weak::upgrade).is_some() {
        return Err("frame source token already registered".into());
    }
    registry.insert(token.to_owned(), Arc::downgrade(&registered));
    Ok(FrameRegistration {
        token: token.to_owned(),
        url: format!("{SCHEME}://{token}"),
        registered,
    })
}

pub fn open_frames(url: &str) -> Result<FrameLease, String> {
    static NEXT_CONSUMER: AtomicU64 = AtomicU64::new(1);
    let token = url
        .split_once("://")
        .filter(|(scheme, token)| *scheme == SCHEME && valid_token(token))
        .map(|(_, token)| token)
        .ok_or("invalid frame source URL")?;
    let registered = lock_registry()
        .get(token)
        .and_then(Weak::upgrade)
        .ok_or("frame source is unavailable")?;
    let lease = FrameLease {
        registered,
        closed: AtomicBool::new(false),
        consumer_id: NEXT_CONSUMER.fetch_add(1, Ordering::Relaxed),
    };
    lease.check()?;
    Ok(lease)
}

pub struct FrameLease {
    registered: Arc<Registered>,
    closed: AtomicBool,
    consumer_id: u64,
}

impl FrameLease {
    pub fn is_control_owner(&self) -> Result<bool, String> {
        self.with_control(|_| Ok(())).map(|result| result.is_some())
    }
    fn with_control<T>(
        &self,
        action: impl FnOnce(&dyn FrameSource) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        self.check()?;
        let owner = self
            .registered
            .control
            .lock()
            .map_err(|_| "frame control lock poisoned")?;
        if owner.as_ref().is_none_or(|(id, _)| *id != self.consumer_id) {
            return Ok(None);
        }
        let result = action(self.registered.source.as_ref())?;
        self.check()?;
        Ok(Some(result))
    }

    pub fn seek_if_owner(&self, position_ms: u64) -> Result<Option<u64>, String> {
        self.with_control(|source| source.seek(position_ms))
    }

    pub fn configure_decoder_if_owner(
        &self,
        device: Option<Arc<dyn DecodeDevice>>,
    ) -> Result<bool, String> {
        self.with_control(|source| source.configure_decoder(device))
            .map(|result| result.is_some())
    }

    pub fn reject_decoder_if_owner(&self, detail: &str) -> Result<bool, String> {
        self.with_control(|source| source.reject_decoder(detail))
            .map(|result| result.is_some())
    }

    pub fn recover_presentation_if_owner(&self, detail: &str) -> Result<bool, String> {
        self.with_control(|source| source.recover_presentation(detail))
            .map(|result| result.is_some())
    }

    pub fn presented_if_owner(&self, generation: u64, serial: u64) -> Result<bool, String> {
        self.with_control(|source| source.presented(generation, serial))
            .map(|result| result.unwrap_or(false))
    }
    fn check(&self) -> Result<(), String> {
        if self.registered.revoked.load(Ordering::Acquire) {
            Err("frame source was revoked".into())
        } else if self.closed.load(Ordering::Acquire) {
            Err("frame consumer was closed".into())
        } else {
            Ok(())
        }
    }

    pub fn info(&self) -> Result<Option<FrameInfo>, String> {
        self.check()?;
        let info = self.registered.source.info();
        self.check()?;
        info
    }

    pub fn native_frame_at(
        &self,
        position_secs: f64,
    ) -> Result<Option<Arc<PresentationFrame>>, String> {
        self.check()?;
        let frame = self.registered.source.native_frame_at(position_secs);
        self.check()?;
        frame
    }

    pub fn configure_decoder(&self, device: Option<Arc<dyn DecodeDevice>>) -> Result<(), String> {
        self.check()?;
        let result = self.registered.source.configure_decoder(device);
        self.check()?;
        result
    }

    pub fn control(&self, control: PlaybackControl) -> Result<(), String> {
        self.check()?;
        let mut owner = self
            .registered
            .control
            .lock()
            .map_err(|_| "frame control lock poisoned")?;
        let result = self.registered.source.control(control);
        if result.is_ok() {
            *owner = Some((self.consumer_id, control));
        }
        self.check()?;
        result
    }

    pub fn control_if_owner(&self, control: PlaybackControl) -> Result<bool, String> {
        self.check()?;
        let mut owner = self
            .registered
            .control
            .lock()
            .map_err(|_| "frame control lock poisoned")?;
        if owner.as_ref().is_none_or(|(id, _)| *id != self.consumer_id) {
            return Ok(false);
        }
        self.registered.source.control(control)?;
        *owner = Some((self.consumer_id, control));
        self.check()?;
        Ok(true)
    }

    pub fn pause_if_owner(&self) -> Result<bool, String> {
        self.check()?;
        let mut owner = self
            .registered
            .control
            .lock()
            .map_err(|_| "frame control lock poisoned")?;
        let Some((_, intent)) = owner.as_mut().filter(|(id, _)| *id == self.consumer_id) else {
            return Ok(false);
        };
        let paused = PlaybackControl {
            playing: false,
            ..*intent
        };
        self.registered.source.control(paused)?;
        *intent = paused;
        self.check()?;
        Ok(true)
    }

    pub fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        self.check()?;
        let result = self.registered.source.position();
        self.check()?;
        result
    }

    pub fn reject_decoder(&self, detail: &str) -> Result<(), String> {
        self.check()?;
        let result = self.registered.source.reject_decoder(detail);
        self.check()?;
        result
    }

    pub fn recover_presentation(&self, detail: &str) -> Result<(), String> {
        self.check()?;
        let result = self.registered.source.recover_presentation(detail);
        self.check()?;
        result
    }

    pub fn frame_at(&self, position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        self.check()?;
        let frame = self.registered.source.frame_at(position_ms);
        self.check()?;
        let frame = frame?;
        if let Some(frame) = &frame {
            let row = frame
                .width
                .checked_mul(BGRA_BYTES_PER_PIXEL)
                .ok_or("invalid BGRA row size")?;
            let bytes = u64::from(frame.stride) * u64::from(frame.height);
            if frame.width == 0
                || frame.height == 0
                || frame.stride < row
                || bytes > frame.pixels.len() as u64
            {
                return Err("invalid BGRA frame dimensions or storage".into());
            }
        }
        Ok(frame)
    }

    pub fn seek(&self, position_ms: u64) -> Result<u64, String> {
        self.check()?;
        let generation = self.registered.source.seek(position_ms);
        self.check()?;
        generation
    }

    pub fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        self.check()?;
        let result = self.registered.source.presented(generation, serial);
        self.check()?;
        result
    }

    pub fn holding(&self) -> Result<bool, String> {
        self.check()?;
        let holding = self.registered.source.holding();
        self.check()?;
        Ok(holding)
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

impl Drop for FrameLease {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;
