use crate::ffi::player;
use hongguo_nsh_video_api::frames::{FrameLease, PlaybackControl, open_frames};
use std::sync::{Arc, atomic::Ordering};

pub(crate) struct DirectSlot {
    pub epoch: u64,
    pub lease: Arc<FrameLease>,
}

impl DirectSlot {
    pub(crate) fn close(self) {
        if let Err(error) = self.lease.pause_if_owner() {
            log::debug!("[Android video] retiring session controls: {error}");
        }
        self.lease.close();
    }
}

pub(crate) fn lease(handle: i64, epoch: i64) -> Result<Arc<FrameLease>, String> {
    let player = player(handle).ok_or("video controller is closed")?;
    if epoch < 0
        || player.stop.load(Ordering::Acquire)
        || player.generation.load(Ordering::Acquire) != epoch as u64
    {
        return Err("video source is no longer current".into());
    }
    player
        .direct
        .lock()
        .expect("Android direct source")
        .as_ref()
        .filter(|slot| slot.epoch == epoch as u64)
        .map(|slot| slot.lease.clone())
        .ok_or_else(|| "direct source is closed".into())
}

pub fn direct_open(handle: i64, epoch: i64, url: &str) -> Result<(), String> {
    let player = player(handle).ok_or("video controller is closed")?;
    let lease = Arc::new(open_frames(url)?);
    {
        let mailbox = player.mailbox.lock().expect("Android video mailbox");
        if epoch < 0
            || player.stop.load(Ordering::Acquire)
            || player.generation.load(Ordering::Acquire) != epoch as u64
            || mailbox
                .props
                .source
                .as_ref()
                .and_then(|source| source.frame_source.as_deref())
                != Some(url)
        {
            return Err("direct source is no longer current".into());
        }
        let mut slot = player.direct.lock().expect("Android direct source");
        if slot.is_some() {
            return Err("direct source is already open".into());
        }
        *slot = Some(DirectSlot {
            epoch: epoch as u64,
            lease,
        });
    }
    player.wake();
    Ok(())
}

pub fn direct_control(
    handle: i64,
    epoch: i64,
    playing: bool,
    volume: f32,
    muted: bool,
    rate: f64,
    claim_owner: bool,
) -> Result<(), String> {
    let lease = lease(handle, epoch)?;
    let control = PlaybackControl {
        playing,
        volume,
        muted,
        rate,
    };
    if claim_owner {
        lease.control(control)?;
    } else if !lease.control_if_owner(control)? {
        return Ok(());
    }
    if let Some(player) = player(handle) {
        let bridge = player.bridge.clone();
        lease.configure_native_clock_if_owner(&|url| {
            crate::native_audio::open(bridge.clone(), url)
        })?;
        player.wake();
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackStatus {
    position_ms: u64,
    duration_ms: Option<u64>,
    is_ended: bool,
    is_buffering: bool,
    video_gap: bool,
}

pub fn direct_status(handle: i64, epoch: i64) -> Result<String, String> {
    let position = lease(handle, epoch)?
        .position()?
        .ok_or("native playback position is not ready")?;
    let status = PlaybackStatus {
        position_ms: (position.position_secs.max(0.0) * 1000.0).round() as u64,
        duration_ms: (position.duration_secs > 0.0)
            .then_some((position.duration_secs * 1000.0).round() as u64),
        is_ended: position.ended,
        is_buffering: position.buffering,
        video_gap: position.video_gap,
    };
    serde_json::to_string(&status).map_err(|error| error.to_string())
}

pub fn direct_is_owner(handle: i64, epoch: i64) -> Result<bool, String> {
    lease(handle, epoch)?.is_control_owner()
}

pub fn direct_seek(handle: i64, epoch: i64, position: i64) -> Result<i64, String> {
    let position = u64::try_from(position).map_err(|_| "negative seek position")?;
    let Some(generation) = lease(handle, epoch)?.seek_if_owner(position)? else {
        return Ok(-1);
    };
    let player = player(handle).ok_or("video controller is closed")?;
    let mailbox = player.mailbox.lock().expect("Android video mailbox");
    if epoch < 0
        || epoch as u64 != player.generation.load(Ordering::Acquire)
        || player.stop.load(Ordering::Acquire)
    {
        return Err("video source is no longer current".into());
    }
    drop(mailbox);
    player.revision.fetch_add(1, Ordering::Release);
    player.wake();
    i64::try_from(generation).map_err(|_| "video generation exceeds native range".into())
}

pub fn direct_presented(
    handle: i64,
    epoch: i64,
    generation: i64,
    serial: i64,
) -> Result<bool, String> {
    if generation < 0 || serial < 0 {
        return Err("negative presentation epoch or serial".into());
    }
    let accepted = lease(handle, epoch)?.presented_if_owner(generation as u64, serial as u64)?;
    if accepted && let Some(player) = player(handle) {
        player.wake();
    }
    Ok(accepted)
}

pub fn direct_close(handle: i64, epoch: i64) {
    let Some(player) = player(handle) else {
        return;
    };
    let old = {
        let mut slot = player.direct.lock().expect("Android direct source");
        if slot
            .as_ref()
            .is_some_and(|slot| epoch >= 0 && slot.epoch == epoch as u64)
        {
            slot.take()
        } else {
            None
        }
    };
    if let Some(slot) = old {
        slot.close();
    }
}
