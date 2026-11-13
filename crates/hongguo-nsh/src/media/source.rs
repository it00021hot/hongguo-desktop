use super::hls_source::{self, Failure, Growing, Published};
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
    Memory(Arc<[u8]>),
    Growing(Growing),
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
    pub(super) fn wait_for_available(
        &self,
        target: f64,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<(), String> {
        match &self.kind {
            SourceKind::Growing(growing) => growing
                .wait_for_available(target, &self.revoked, cancelled)
                .map_err(|error| error.to_string()),
            _ => Err("media source does not have growing readiness".into()),
        }
    }

    pub(super) fn available_duration(&self) -> Result<Option<f64>, String> {
        if self.revoked.load(Ordering::Acquire) {
            return Err("media lease has been revoked".into());
        }
        match &self.kind {
            SourceKind::Growing(growing) => growing
                .available_duration()
                .map(Some)
                .map_err(|error| error.to_string()),
            _ => Ok(None),
        }
    }
    pub(super) fn with_memory(&self, bytes: Arc<[u8]>) -> Self {
        Self {
            kind: SourceKind::Memory(bytes),
            revoked: self.revoked.clone(),
        }
    }
    pub(super) fn file(path: &Path) -> Result<Self, String> {
        if let Some(snapshot) = hongguo_protocol::grow::snapshot(path) {
            use hongguo_protocol::grow::GrowthState;
            match snapshot.state {
                GrowthState::Open => return Ok(Self::new(SourceKind::Growing(Growing::new(path)))),
                GrowthState::Failed(message) => return Err(message),
                GrowthState::Cancelled => return Err("growing media was cancelled".into()),
                GrowthState::Done => {}
            }
        }
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

    pub(super) fn is_growing(&self) -> bool {
        matches!(&self.kind, SourceKind::Growing(_))
    }

    pub(super) fn is_complete(&self) -> Result<bool, String> {
        if self.revoked.load(Ordering::Acquire) {
            return Err("media lease has been revoked".into());
        }
        match &self.kind {
            SourceKind::Growing(growing) => hls_source::state(&growing.path)
                .map(|state| state.state == hongguo_protocol::grow::GrowthState::Done)
                .map_err(|error| error.to_string()),
            _ => Ok(true),
        }
    }

    pub(super) fn hls_snapshot(
        &self,
        cancelled: &AtomicBool,
        wait: bool,
    ) -> Result<Published, Failure> {
        match &self.kind {
            SourceKind::Growing(growing) => growing.snapshot(&self.revoked, cancelled, wait),
            _ => Err(Failure::Invalid(
                "source is not a growing fragmented movie".into(),
            )),
        }
    }

    pub(super) fn len(&self) -> Option<u64> {
        match &self.kind {
            SourceKind::Memory(bytes) => Some(bytes.len() as u64),
            SourceKind::File { len, .. } | SourceKind::Online { len, .. } => Some(*len),
            SourceKind::Growing(growing) => hongguo_protocol::grow::completed_len(&growing.path),
        }
    }

    pub(super) fn read(
        &self,
        start: u64,
        count: u64,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        self.read_with(start, count, || cancelled.load(Ordering::Acquire))
    }

    pub(super) fn read_with(
        &self,
        start: u64,
        count: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        let interrupted = || self.revoked.load(Ordering::Acquire) || cancelled();
        if interrupted() {
            return Err("media read cancelled".into());
        }
        match &self.kind {
            SourceKind::Memory(bytes) => {
                let end = start
                    .checked_add(count)
                    .ok_or("media memory range overflow")?;
                bytes
                    .get(start as usize..end as usize)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| "media memory range is out of bounds".into())
            }
            SourceKind::Growing(growing) => {
                hls_source::state(&growing.path).map_err(|error| error.to_string())?;
                let bytes = hongguo_protocol::grow::read_cancellable(
                    &growing.path,
                    start,
                    count,
                    interrupted,
                )?;
                hls_source::state(&growing.path).map_err(|error| error.to_string())?;
                if interrupted() {
                    return Err("media read cancelled".into());
                }
                Ok(bytes)
            }
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
