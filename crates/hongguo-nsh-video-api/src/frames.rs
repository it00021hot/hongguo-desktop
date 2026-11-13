//! Process-local BGRA frame sources. Registration owns the shared session;
//! closing a consumer only retires that consumer.
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, Ordering},
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

pub trait FrameSource: Send + Sync {
    fn info(&self) -> Result<Option<FrameInfo>, String>;
    fn frame_at(&self, position_ms: u64) -> Result<Option<Arc<PixelFrame>>, String>;
    fn seek(&self, position_ms: u64) -> Result<u64, String>;
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String>;
    fn holding(&self) -> bool;
    fn close(&self);
}

struct Registered {
    source: Arc<dyn FrameSource>,
    revoked: AtomicBool,
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
    };
    lease.check()?;
    Ok(lease)
}

pub struct FrameLease {
    registered: Arc<Registered>,
    closed: AtomicBool,
}

impl FrameLease {
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
