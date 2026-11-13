use block2::RcBlock;
use hongguo_nsh_video_api::ErrorCode;
use objc2::{
    Encode, Encoding, RefEncode, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSString, NSURL};
use std::{
    fmt,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVPlayerItemDidPlayToEndTimeNotification: *const AnyObject;
}

#[derive(Debug)]
pub struct OutputError {
    code: ErrorCode,
    detail: String,
}

impl OutputError {
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub(crate) fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for OutputError {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MediaTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

// SAFETY: CoreMedia CMTime has this documented C representation and ObjC encoding.
unsafe impl Encode for MediaTime {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING],
    );
}

// SAFETY: Pointers to CMTime encode as pointers to its documented C structure.
unsafe impl RefEncode for MediaTime {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

impl MediaTime {
    pub(crate) fn milliseconds(self) -> Option<u64> {
        const VALID: u32 = 1;
        const NON_NUMERIC: u32 = 4 | 8 | 16;
        if self.flags & VALID == 0 || self.flags & NON_NUMERIC != 0 || self.timescale <= 0 {
            return None;
        }
        let value = i128::from(self.value) * 1000 / i128::from(self.timescale);
        u64::try_from(value).ok()
    }
}

pub(crate) struct AudioOutput {
    player: Retained<AnyObject>,
    item: Retained<AnyObject>,
    notifications: Retained<AnyObject>,
    end_observer: Retained<AnyObject>,
    ended: Arc<AtomicBool>,
    held: Arc<AtomicBool>,
}

impl AudioOutput {
    pub(crate) fn control(&self) -> crate::control::PlaybackControl {
        crate::control::PlaybackControl(self.player.clone(), self.held.clone())
    }
    pub(crate) fn hold(&self, held: bool) -> bool {
        let previous = self.held.swap(held, Ordering::AcqRel);
        if held {
            self.control().pause();
        }
        previous
    }

    pub(crate) fn ready(&self) -> Result<bool, OutputError> {
        // SAFETY: AVPlayerItem exposes its status and NSError on retained objects.
        unsafe {
            let status: isize = msg_send![&self.item, status];
            if status == 2 {
                let error: Option<Retained<AnyObject>> = msg_send![&self.item, error];
                let detail = error
                    .map(|error| {
                        let description: Retained<NSString> =
                            msg_send![&error, localizedDescription];
                        description.to_string()
                    })
                    .unwrap_or_else(|| "AVPlayerItem failed without NSError".into());
                Err(OutputError::new(ErrorCode::DecoderFailed, detail))
            } else {
                Ok(status == 1)
            }
        }
    }
    pub fn seek(&self, position_ms: u64) -> Result<std::sync::mpsc::Receiver<bool>, OutputError> {
        self.ended.store(false, Ordering::Release);
        let value = i64::try_from(position_ms).map_err(|_| {
            OutputError::new(
                ErrorCode::InvalidProps,
                "seek position exceeds native CMTime range",
            )
        })?;
        let target = MediaTime {
            value,
            timescale: 1_000,
            flags: 1,
            epoch: 0,
        };
        let zero = MediaTime {
            value: 0,
            timescale: 1,
            flags: 1,
            epoch: 0,
        };
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let completed = RcBlock::new(move |finished: Bool| {
            let _ = send.send(finished.as_bool());
        });
        // SAFETY: AVPlayer copies the completion block; it captures only a thread-safe channel sender.
        unsafe {
            let _: () = msg_send![&self.item, cancelPendingSeeks];
            let _: () = msg_send![&self.player, seekToTime: target, toleranceBefore: zero, toleranceAfter: zero, completionHandler: &*completed];
        }
        Ok(receive)
    }

    pub(crate) fn open_audio(url: &str) -> Result<Self, OutputError> {
        let url = NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| OutputError::new(ErrorCode::InvalidProps, "invalid media URL"))?;
        let item_class = class(c"AVPlayerItem")?;
        let player_class = class(c"AVPlayer")?;
        let notification_class = class(c"NSNotificationCenter")?;
        // SAFETY: Public AVFoundation constructors for the audio-only source.
        let (item, player) = unsafe {
            let item: Retained<AnyObject> = msg_send![item_class, playerItemWithURL: &*url];
            let player: Retained<AnyObject> = msg_send![player_class, playerWithPlayerItem: &*item];
            (item, player)
        };
        let ended = Arc::new(AtomicBool::new(false));
        let notify_end = ended.clone();
        let callback =
            RcBlock::new(move |_: NonNull<AnyObject>| notify_end.store(true, Ordering::Release));
        // SAFETY: The public notification is scoped to this item; its callback touches only an atomic flag.
        let (notifications, end_observer) = unsafe {
            let center: Retained<AnyObject> = msg_send![notification_class, defaultCenter];
            let observer: Retained<AnyObject> = msg_send![&center,
                addObserverForName: AVPlayerItemDidPlayToEndTimeNotification,
                object: &*item,
                queue: std::ptr::null_mut::<AnyObject>(),
                usingBlock: &*callback];
            (center, observer)
        };
        Ok(Self {
            player,
            item,
            notifications,
            end_observer,
            ended,
            held: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn position_ms(&self) -> Option<u64> {
        // SAFETY: Public CMTime getter on a retained AVPlayer.
        let time: MediaTime = unsafe { msg_send![&self.player, currentTime] };
        time.milliseconds()
    }

    pub fn take_ended(&self) -> bool {
        self.ended.swap(false, Ordering::AcqRel)
    }

    pub fn buffering(&self) -> bool {
        const WAITING: isize = 1;
        // SAFETY: Public AVPlayerTimeControlStatus on the retained player.
        let status: isize = unsafe { msg_send![&self.player, timeControlStatus] };
        status == WAITING
    }

    pub fn duration_ms(&self) -> Option<u64> {
        // SAFETY: Public CMTime getter on this retained AVPlayerItem.
        let time: MediaTime = unsafe { msg_send![&self.item, duration] };
        time.milliseconds()
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        // SAFETY: Stop callbacks and remove the output before releasing this instance's objects.
        unsafe {
            let _: () = msg_send![&self.notifications, removeObserver: &*self.end_observer];
            let _: () = msg_send![&self.player, pause];
            let _: () = msg_send![&self.item, cancelPendingSeeks];
            let _: () = msg_send![&self.player, replaceCurrentItemWithPlayerItem: std::ptr::null_mut::<AnyObject>()];
        }
    }
}

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, OutputError> {
    AnyClass::get(name).ok_or_else(|| {
        OutputError::new(
            ErrorCode::DecoderUnavailable,
            format!("AVFoundation class {name:?} unavailable"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::MediaTime;

    #[test]
    fn media_positions_require_finite_nonnegative_times() {
        let valid = MediaTime {
            value: 750,
            timescale: 600,
            flags: 1,
            epoch: 0,
        };
        assert_eq!(valid.milliseconds(), Some(1_250));
        for invalid in [
            MediaTime { flags: 0, ..valid },
            MediaTime { flags: 5, ..valid },
            MediaTime { flags: 9, ..valid },
            MediaTime { flags: 17, ..valid },
            MediaTime {
                timescale: 0,
                ..valid
            },
            MediaTime {
                value: -600,
                ..valid
            },
        ] {
            assert_eq!(invalid.milliseconds(), None, "{invalid:?}");
        }
    }
}
