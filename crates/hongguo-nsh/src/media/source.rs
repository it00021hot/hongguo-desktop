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
    audio_view: Option<Arc<hongguo_video::audio_view::AudioView>>,
    only_audio: bool,
    pub(super) codec: Option<String>,
    pub(super) precision_extension: bool,
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
    pub(super) fn prepare(mut self) -> Result<(Self, String), String> {
        let length = self.len().ok_or("media length is unavailable")?;
        let (offset, moov) = hongguo_bytevc2::read_moov_at(length, |offset, count, bytes| {
            *bytes = self
                .read_with(offset, count, &|| false)
                .map_err(hongguo_common::AppError::Media)?;
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        self.audio_view = hongguo_video::audio_view::AudioView::from_moov(offset, &moov)
            .map_err(|error| error.to_string())?
            .map(Arc::new);
        let track = hongguo_video::sample_table::collect_tracks(&moov)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|track| track.is_video)
            .ok_or_else(|| "media source has no video track".to_string())?;
        if cfg!(any(target_os = "windows", target_os = "android"))
            && matches!(track.codec.as_str(), "hvc1" | "hev1")
        {
            let (configuration, color) = hongguo_playback::video_configuration(
                &moov,
                &track,
                hongguo_playback::TrackCodec::Hevc,
            )
            .map_err(|error| error.to_string())?;
            self.precision_extension = configuration.get(17).is_some_and(|depth| depth & 7 != 0)
                || matches!(color.transfer, Some(16 | 18));
        }
        let codec = track.codec;
        self.codec = Some(codec.clone());
        Ok((self, codec))
    }

    pub(super) fn audio_view(&self) -> Option<Self> {
        self.audio_view.as_ref()?;
        let mut source = self.clone();
        source.only_audio = true;
        Some(source)
    }

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
            audio_view: None,
            only_audio: false,
            codec: None,
            precision_extension: false,
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
        let mut bytes = match &self.kind {
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
        }?;
        if self.only_audio
            && let Some(view) = &self.audio_view
        {
            view.apply(start, &mut bytes);
        }
        Ok(bytes)
    }
}
