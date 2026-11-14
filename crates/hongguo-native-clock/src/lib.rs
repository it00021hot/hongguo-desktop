//! Native audio stays in the platform player; video codec extensions read its clock.

#[cfg(test)]
mod tests;
mod worker;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{SyncSender, sync_channel},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    pub playing: bool,
    pub rate: f64,
    pub volume: f32,
    pub muted: bool,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            playing: false,
            rate: 1.0,
            volume: 1.0,
            muted: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Position {
    pub seconds: f64,
    pub ready: bool,
    pub buffering: bool,
    pub seeking: bool,
    pub ended: bool,
}

pub trait Clock: Send + Sync {
    fn position(&self) -> Result<Position, String>;
    fn control(&self, control: Control) -> Result<(), String>;
    fn seek(&self, seconds: f64) -> Result<(), String>;
    fn close(&self);
}

pub trait NativePlayer: 'static {
    fn position(&mut self) -> Result<Position, String>;
    fn control(&mut self, control: Control) -> Result<(), String>;
    fn seek(&mut self, seconds: f64) -> Result<(), String>;
}

struct Mailbox {
    control: Control,
    revision: u64,
    seek: Option<f64>,
}

struct Shared {
    mailbox: Mutex<Mailbox>,
    position: Mutex<Result<Position, String>>,
    closed: AtomicBool,
}

pub struct NativeClock {
    shared: Arc<Shared>,
    wake: SyncSender<()>,
}

impl NativeClock {
    pub fn open<P: NativePlayer>(
        factory: impl FnOnce() -> Result<P, String> + Send + 'static,
    ) -> Result<Arc<Self>, String> {
        let (wake, receiver) = sync_channel(1);
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox {
                control: Control::default(),
                revision: 0,
                seek: None,
            }),
            position: Mutex::new(Ok(Position::default())),
            closed: AtomicBool::new(false),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("video-native-audio".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker::run(&worker, receiver, factory)
                }));
                let error = match result {
                    Ok(Err(error)) => Some(error),
                    Err(_) => Some("native audio worker panicked".into()),
                    Ok(Ok(())) => None,
                };
                if let Some(error) = error {
                    *worker
                        .position
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Err(error);
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self { shared, wake }))
    }

    fn check(&self) -> Result<(), String> {
        if self.shared.closed.load(Ordering::Acquire) {
            Err("native playback clock is closed".into())
        } else {
            Ok(())
        }
    }

    fn wake(&self) {
        let _ = self.wake.try_send(());
    }
}

impl Clock for NativeClock {
    fn position(&self) -> Result<Position, String> {
        self.check()?;
        self.wake();
        self.shared
            .position
            .lock()
            .map_err(|_| "native clock state lock poisoned")?
            .clone()
    }

    fn control(&self, control: Control) -> Result<(), String> {
        self.check()?;
        if !control.rate.is_finite()
            || control.rate <= 0.0
            || !control.volume.is_finite()
            || !(0.0..=1.0).contains(&control.volume)
        {
            return Err("invalid native playback controls".into());
        }
        {
            let mut mailbox = self
                .shared
                .mailbox
                .lock()
                .map_err(|_| "native clock mailbox poisoned")?;
            if mailbox.control == control {
                return Ok(());
            }
            mailbox.control = control;
            mailbox.revision = mailbox
                .revision
                .checked_add(1)
                .ok_or("native clock revision exhausted")?;
        }
        self.wake();
        Ok(())
    }

    fn seek(&self, seconds: f64) -> Result<(), String> {
        self.check()?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("invalid native seek position".into());
        }
        self.shared
            .mailbox
            .lock()
            .map_err(|_| "native clock mailbox poisoned")?
            .seek = Some(seconds);
        {
            let mut state = self
                .shared
                .position
                .lock()
                .map_err(|_| "native clock state lock poisoned")?;
            if let Ok(position) = state.as_mut() {
                position.seeking = true;
                position.ready = false;
            }
        }
        self.wake();
        Ok(())
    }

    fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        self.wake();
    }
}

impl Drop for NativeClock {
    fn drop(&mut self) {
        self.close();
    }
}
