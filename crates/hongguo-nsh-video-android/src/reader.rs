//! JNI data callbacks never retain a controller lock while waiting for bytes.
use crate::ffi::player;
use hongguo_nsh_video_api::read_source::{self, ReadLease, ReadResult};
use std::sync::{Arc, atomic::Ordering};

pub(crate) struct ReaderSlot {
    pub generation: u64,
    pub lease: Arc<ReadLease>,
}

pub fn reader_open(handle: i64, generation: i64, url: &str) -> Result<bool, String> {
    if !read_source::is_read_source_url(url) {
        return Ok(false);
    }
    let player = player(handle).ok_or("media controller is closed")?;
    let lease = Arc::new(read_source::open_source(url).map_err(|error| error.to_string())?);
    let old = {
        let mailbox = player.mailbox.lock().expect("Android video mailbox");
        if player.stop.load(Ordering::Acquire)
            || generation < 0
            || generation as u64 != player.generation.load(Ordering::Acquire)
            || mailbox
                .props
                .source
                .as_ref()
                .is_none_or(|source| source.url != url)
        {
            lease.close();
            return Err("media source generation is no longer current".into());
        }
        let mut reader = player.reader.lock().expect("Android media reader");
        if reader.is_some() {
            lease.close();
            return Err("media reader already open for this generation".into());
        }
        reader.replace(ReaderSlot {
            generation: generation as u64,
            lease,
        })
    };
    if let Some(old) = old {
        old.lease.close();
    }
    Ok(true)
}

fn lease(handle: i64, generation: i64) -> Result<Arc<ReadLease>, String> {
    let player = player(handle).ok_or("media controller is closed")?;
    if player.stop.load(Ordering::Acquire)
        || generation < 0
        || generation as u64 != player.generation.load(Ordering::Acquire)
    {
        return Err("media source generation is no longer current".into());
    }
    let reader = player.reader.lock().expect("Android media reader");
    reader
        .as_ref()
        .filter(|reader| reader.generation == generation as u64)
        .map(|reader| reader.lease.clone())
        .ok_or_else(|| "media reader is closed".into())
}

pub fn reader_read(
    handle: i64,
    generation: i64,
    offset: i64,
    limit: i32,
) -> Result<Option<Vec<u8>>, String> {
    let offset = u64::try_from(offset).map_err(|_| "negative media offset")?;
    let limit = usize::try_from(limit).map_err(|_| "negative media read length")?;
    if limit == 0 {
        return Err("empty media read must be handled by the Java buffer boundary".into());
    }
    let result = match lease(handle, generation)?
        .read_at(offset, limit)
        .map_err(|error| error.to_string())?
    {
        ReadResult::Data(bytes) => Ok(Some(bytes)),
        ReadResult::Eof => Ok(None),
    };
    // Source replacement advances the epoch before retiring the old slot. Check
    // again after a blocking read so that short retirement interval cannot leak
    // a late result into a platform callback.
    lease(handle, generation)?;
    result
}

pub fn reader_size(handle: i64, generation: i64) -> Result<i64, String> {
    let size = lease(handle, generation)?
        .size()
        .map_err(|error| error.to_string())?
        .map(|size| i64::try_from(size).map_err(|_| "media length exceeds native range".into()))
        .transpose()
        .map(|size| size.unwrap_or(-1));
    lease(handle, generation)?;
    size
}

pub fn reader_close(handle: i64, generation: i64) {
    let Some(player) = player(handle) else {
        return;
    };
    let old = {
        let mut reader = player.reader.lock().expect("Android media reader");
        if reader
            .as_ref()
            .is_some_and(|reader| generation >= 0 && reader.generation == generation as u64)
        {
            reader.take()
        } else {
            None
        }
    };
    if let Some(old) = old {
        old.lease.close();
    }
}
