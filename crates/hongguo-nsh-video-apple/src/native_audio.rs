#[cfg(all(target_os = "macos", feature = "test-support"))]
pub(crate) mod probe;
mod time;

use hongguo_native_clock::{Clock, Control, NativeClock, NativePlayer, Position};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSString, NSURL};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use time::Time;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVAudioTimePitchAlgorithmSpectral: *const AnyObject;
}

pub(crate) fn open(url: String) -> Result<Arc<dyn Clock>, String> {
    NativeClock::open(move || objc2::rc::autoreleasepool(|_| Player::open(&url)))
        .map(|clock| clock as Arc<dyn Clock>)
}

struct Player {
    player: Retained<AnyObject>,
    item: Retained<AnyObject>,
    requested_seek: Arc<AtomicU64>,
    completed_seek: Arc<AtomicU64>,
    failed_seek: Arc<AtomicU64>,
    pending_seek: Option<(u64, f64)>,
}

impl Player {
    fn open(url: &str) -> Result<Self, String> {
        let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or("invalid audio URL")?;
        let item_class = AnyClass::get(c"AVPlayerItem").ok_or("AVPlayerItem unavailable")?;
        let player_class = AnyClass::get(c"AVPlayer").ok_or("AVPlayer unavailable")?;
        let item: Retained<AnyObject> = unsafe { msg_send![item_class, playerItemWithURL: &*url] };
        unsafe {
            let _: () =
                msg_send![&*item, setAudioTimePitchAlgorithm: AVAudioTimePitchAlgorithmSpectral];
        }
        let player: Retained<AnyObject> =
            unsafe { msg_send![player_class, playerWithPlayerItem: &*item] };
        Ok(Self {
            player,
            item,
            requested_seek: Arc::new(AtomicU64::new(0)),
            completed_seek: Arc::new(AtomicU64::new(0)),
            failed_seek: Arc::new(AtomicU64::new(0)),
            pending_seek: None,
        })
    }

    fn read_position(&mut self) -> Result<Position, String> {
        let status: isize = unsafe { msg_send![&*self.item, status] };
        if status == 2 {
            let error: Option<Retained<AnyObject>> = unsafe { msg_send![&*self.item, error] };
            let detail: Option<Retained<NSString>> = error
                .as_ref()
                .map(|error| unsafe { msg_send![&**error, localizedDescription] });
            return Err(
                detail.map_or_else(|| "native audio failed".into(), |detail| detail.to_string())
            );
        }
        if status == 1
            && let Some((epoch, seconds)) = self.pending_seek.take()
        {
            self.begin_seek(epoch, seconds);
        }
        let requested = self.requested_seek.load(Ordering::Acquire);
        if requested != 0 && self.failed_seek.load(Ordering::Acquire) == requested {
            return Err("native audio seek did not complete".into());
        }
        let current: Time = unsafe { msg_send![&*self.player, currentTime] };
        let duration: Time = unsafe { msg_send![&*self.item, duration] };
        let seconds = current.value().unwrap_or(0.0).max(0.0);
        let transport: isize = unsafe { msg_send![&*self.player, timeControlStatus] };
        let empty: Bool = unsafe { msg_send![&*self.item, isPlaybackBufferEmpty] };
        let seeking = requested != self.completed_seek.load(Ordering::Acquire);
        Ok(Position {
            seconds,
            ready: status == 1 && !seeking,
            buffering: transport == 1 || (status != 1 || empty.as_bool()),
            seeking,
            ended: duration
                .value()
                .is_some_and(|duration| duration > 0.0 && seconds + 0.000_001 >= duration),
        })
    }

    fn begin_seek(&self, epoch: u64, seconds: f64) {
        let requested = self.requested_seek.clone();
        let completed = self.completed_seek.clone();
        let failed = self.failed_seek.clone();
        let completion = block2::RcBlock::new(move |success: Bool| {
            if requested.load(Ordering::Acquire) == epoch {
                if success.as_bool() {
                    completed.store(epoch, Ordering::Release);
                } else {
                    failed.store(epoch, Ordering::Release);
                }
            }
        });
        objc2::rc::autoreleasepool(|_| unsafe {
            let _: () = msg_send![&*self.player, seekToTime: Time::seconds(seconds),
                toleranceBefore: Time::ZERO, toleranceAfter: Time::ZERO,
                completionHandler: &*completion];
        });
    }
}

impl NativePlayer for Player {
    fn position(&mut self) -> Result<Position, String> {
        objc2::rc::autoreleasepool(|_| self.read_position())
    }

    fn control(&mut self, control: Control) -> Result<(), String> {
        objc2::rc::autoreleasepool(|_| unsafe {
            let _: () = msg_send![&*self.player, setVolume: control.volume];
            let _: () = msg_send![&*self.player, setMuted: Bool::new(control.muted)];
            if control.playing {
                let _: () = msg_send![&*self.player, playImmediatelyAtRate: control.rate as f32];
            } else {
                let _: () = msg_send![&*self.player, pause];
            }
            Ok(())
        })
    }

    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        let previous = self.requested_seek.load(Ordering::Acquire);
        let epoch = previous
            .checked_add(1)
            .ok_or("native audio seek epoch exhausted")?;
        self.requested_seek.store(epoch, Ordering::Release);
        self.pending_seek = Some((epoch, seconds));
        Ok(())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.player, pause];
            let _: () = msg_send![&*self.player, replaceCurrentItemWithPlayerItem: std::ptr::null::<AnyObject>()];
        }
    }
}
