use crate::ffi::player;
use hongguo_nsh_video_api::frames::{FrameLease, open_frames};
use std::sync::{Arc, atomic::Ordering};

pub(crate) struct DirectSlot {
    pub epoch: u64,
    pub lease: Arc<FrameLease>,
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
        player.audio_position.store(0, Ordering::Release);
        player.audio_seeking.store(false, Ordering::Release);
    }
    player.wake();
    Ok(())
}

pub fn direct_clock(handle: i64, epoch: i64, position: i64, seeking: bool) {
    if position < 0 {
        return;
    }
    if let Some(player) = player(handle) {
        let mailbox = player.mailbox.lock().expect("Android video mailbox");
        if epoch < 0
            || epoch as u64 != player.generation.load(Ordering::Acquire)
            || player.stop.load(Ordering::Acquire)
        {
            return;
        }
        player
            .audio_position
            .store(position as u64, Ordering::Release);
        player.audio_seeking.store(seeking, Ordering::Release);
        drop(mailbox);
        player.wake();
    }
}

pub fn direct_seek(handle: i64, epoch: i64, position: i64) -> Result<i64, String> {
    let position = u64::try_from(position).map_err(|_| "negative seek position")?;
    let generation = lease(handle, epoch)?.seek(position)?;
    let player = player(handle).ok_or("video controller is closed")?;
    let mailbox = player.mailbox.lock().expect("Android video mailbox");
    if epoch < 0
        || epoch as u64 != player.generation.load(Ordering::Acquire)
        || player.stop.load(Ordering::Acquire)
    {
        return Err("video source is no longer current".into());
    }
    player.audio_position.store(position, Ordering::Release);
    player.audio_seeking.store(false, Ordering::Release);
    drop(mailbox);
    player.wake();
    i64::try_from(generation).map_err(|_| "video generation exceeds native range".into())
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
        slot.lease.close();
    }
}
