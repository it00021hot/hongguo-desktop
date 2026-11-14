use super::record::{Recorder, host_time_seconds};
use hongguo_common::{AppError, AppResult};
use hongguo_media_types::{DecodeDevice, FrameImage, PresentationFrame};
use hongguo_nsh_video_api::frames::{
    FrameInfo, FrameSource, PixelFrame, PlaybackControl, PlaybackPosition,
};
use hongguo_playback::{PlaybackSession, ReadAt};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    os::unix::fs::FileExt,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

pub struct FileSource(pub File);

impl ReadAt for FileSource {
    fn length(&self) -> AppResult<u64> {
        Ok(self.0.metadata()?.len())
    }

    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        if length > MAX_READ_BYTES {
            return Err(AppError::Media(
                "probe source read exceeds byte budget".into(),
            ));
        }
        bytes.resize(length as usize, 0);
        self.0.read_exact_at(bytes, offset)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Selected {
    pts: f64,
    duration: f64,
}

#[derive(Default)]
struct Frames {
    selected: HashMap<(u64, u64), Selected>,
    presented: HashSet<(u64, u64)>,
}

pub struct Source {
    pub session: Arc<PlaybackSession>,
    pub recorder: Arc<Recorder>,
    frames: Mutex<Frames>,
}

impl Source {
    pub fn new(session: Arc<PlaybackSession>, recorder: Arc<Recorder>) -> Self {
        Self {
            session,
            recorder,
            frames: Mutex::new(Frames::default()),
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        let frames = self.frames.lock().unwrap();
        (frames.selected.len(), frames.presented.len())
    }
}

impl FrameSource for Source {
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        self.session
            .info()
            .map(|info| {
                info.map(|info| FrameInfo {
                    width: info.width,
                    height: info.height,
                })
            })
            .map_err(|error| error.to_string())
    }

    fn frame_at(&self, _: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        Ok(None)
    }

    fn native_frame_at(&self, position: f64) -> Result<Option<Arc<PresentationFrame>>, String> {
        let frame = self
            .session
            .frame_at(position)
            .map_err(|error| error.to_string())?;
        if let Some(frame) = &frame {
            let selected = Selected {
                pts: frame.pts_secs,
                duration: frame.duration_secs,
            };
            let is_new = self
                .frames
                .lock()
                .unwrap()
                .selected
                .insert((frame.generation, frame.serial), selected)
                .is_none();
            if is_new {
                let descriptor = frame.image.descriptor();
                let backend = match &frame.image {
                    FrameImage::Cpu(_) => "software".to_owned(),
                    FrameImage::Hardware(frame) => format!("{:?}", frame.handle().kind),
                };
                self.recorder.event(json!({"event":"frameSelected", "generation":frame.generation,
                    "serial":frame.serial, "ptsSeconds":selected.pts, "durationSeconds":selected.duration,
                    "requestedPositionSeconds":position, "backend":backend,
                    "width":descriptor.width,"height":descriptor.height,"bitDepth":descriptor.bit_depth}));
            }
        }
        Ok(frame)
    }

    fn configure_decoder(&self, device: Option<Arc<dyn DecodeDevice>>) -> Result<(), String> {
        self.recorder.event(json!({"event":"decoderConfigured", "kind":device.as_ref().map(|d|format!("{:?}",d.kind())), "deviceGeneration":device.as_ref().map(|d|d.generation())}));
        self.session
            .configure_decoder(device)
            .map_err(|error| error.to_string())
    }

    fn reject_decoder(&self, detail: &str) -> Result<(), String> {
        self.recorder
            .event(json!({"event":"decoderRejected","detail":detail}));
        self.session
            .reject_decoder(detail.to_owned())
            .map_err(|error| error.to_string())
    }

    fn recover_presentation(&self, detail: &str) -> Result<(), String> {
        self.recorder
            .event(json!({"event":"presentationRecovery","detail":detail}));
        self.session
            .recover_presentation(detail.to_owned())
            .map_err(|error| error.to_string())
    }

    fn control(&self, control: PlaybackControl) -> Result<(), String> {
        self.session
            .set_volume(control.volume, control.muted)
            .map_err(|error| error.to_string())?;
        self.session
            .set_rate(control.rate)
            .map_err(|error| error.to_string())?;
        self.session
            .set_playing(control.playing)
            .map_err(|error| error.to_string())
    }

    fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        self.session
            .position()
            .map(|p| {
                Some(PlaybackPosition {
                    position_secs: p.position_secs,
                    duration_secs: p.duration_secs,
                    ended: p.ended,
                    buffering: p.buffering,
                    video_gap: p.video_gap,
                })
            })
            .map_err(|error| error.to_string())
    }

    fn seek(&self, position_ms: u64) -> Result<u64, String> {
        let generation = self
            .session
            .seek(position_ms as f64 / 1000.0)
            .map_err(|error| error.to_string())?;
        self.recorder
            .event(json!({"event":"seekApplied","targetMs":position_ms,"generation":generation}));
        Ok(generation)
    }

    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        let selected = {
            let mut frames = self.frames.lock().unwrap();
            if frames.presented.insert((generation, serial)) {
                frames.selected.get(&(generation, serial)).copied()
            } else {
                None
            }
        };
        let query_started = host_time_seconds();
        let position = self.session.position().map_err(|error| error.to_string())?;
        let query_finished = host_time_seconds();
        let released = self
            .session
            .presented(generation, serial)
            .map_err(|error| error.to_string())?;
        if let Some(frame) = selected {
            self.recorder
                .event(json!({"event":"framePresentedAck", "generation":generation,
                "serial":serial,"ptsSeconds":frame.pts,"durationSeconds":frame.duration,
                "pcmPositionSeconds":position.position_secs,"buffering":position.buffering,
                "pcmQueryStartedHostSeconds":query_started,"pcmQueryFinishedHostSeconds":query_finished,
                "cpuObservedPcmMinusPtsMs":(position.position_secs-frame.pts)*1000.0,
                "releasedHold":released}));
        }
        Ok(released)
    }

    fn holding(&self) -> bool {
        self.session.holding()
    }
    fn close(&self) {
        self.session.close();
    }
}
