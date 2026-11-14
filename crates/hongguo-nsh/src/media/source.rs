use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone)]
pub(super) struct Source {
    kind: SourceKind,
    revoked: Arc<AtomicBool>,
}

#[derive(Clone)]
enum SourceKind {
    File {
        file: Arc<Mutex<File>>,
        len: u64,
    },
    Online {
        vid: String,
        definition: u32,
        len: u64,
    },
}

impl Source {
    pub(super) fn file(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err("media source is not a nonempty file".into());
        }
        Ok(Self::new(SourceKind::File {
            file: Arc::new(Mutex::new(file)),
            len: metadata.len(),
        }))
    }

    pub(super) fn online(vid: String, definition: u32) -> Result<Self, String> {
        let len = hongguo_stream::total_len(&vid, definition)
            .filter(|len| *len > 0)
            .ok_or("media stream is not ready")?;
        Ok(Self::new(SourceKind::Online {
            vid,
            definition,
            len,
        }))
    }

    fn new(kind: SourceKind) -> Self {
        Self {
            kind,
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    pub(super) fn len(&self) -> Option<u64> {
        match &self.kind {
            SourceKind::File { len, .. } | SourceKind::Online { len, .. } => Some(*len),
        }
    }

    pub(super) fn read(
        &self,
        start: u64,
        count: u64,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        self.read_with(start, count, &|| cancelled.load(Ordering::Acquire))
    }

    pub(super) fn read_with(
        &self,
        start: u64,
        count: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        let interrupted = || self.revoked.load(Ordering::Acquire) || cancelled();
        if interrupted() {
            return Err("media read cancelled".into());
        }
        match &self.kind {
            SourceKind::File { file, .. } => {
                let mut file = file.lock().map_err(|_| "media file lock poisoned")?;
                if interrupted() {
                    return Err("media read cancelled".into());
                }
                file.seek(SeekFrom::Start(start))
                    .map_err(|error| error.to_string())?;
                let mut bytes = vec![0; count as usize];
                file.read_exact(&mut bytes)
                    .map_err(|error| error.to_string())?;
                Ok(bytes)
            }
            SourceKind::Online {
                vid, definition, ..
            } => hongguo_stream::read_range_cancellable(
                vid,
                *definition,
                start,
                Some(count),
                interrupted,
            )
            .map(|chunk| chunk.bytes)
            .map_err(|error| error.to_string()),
        }
    }
}
