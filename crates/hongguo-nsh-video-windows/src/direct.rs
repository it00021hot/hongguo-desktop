//! One consumer of the shared native decoder, PCM output and presentation clock.
use crate::{VideoError, presentation::Presentation, seek_gate::SeekGate};
use hongguo_media_types::{DecodeDevice, PresentationFrame};
use hongguo_nsh_video_api::{
    ErrorCode, VideoProps,
    frames::{FrameInfo, FrameLease, PlaybackControl, PlaybackPosition, open_frames},
};
use std::sync::Arc;
use windows::Win32::Graphics::{Direct2D::ID2D1DeviceContext, Direct3D11::ID3D11Device};

pub(crate) struct Direct {
    lease: FrameLease,
    uploaded: Option<Arc<PresentationFrame>>,
    presentations: Vec<Presentation>,
    last_submission: Option<(u64, u64)>,
    gate: SeekGate,
    configured: bool,
}

impl Direct {
    pub fn open(
        handle: &str,
        device: Arc<dyn DecodeDevice>,
        props: &VideoProps,
    ) -> Result<Self, VideoError> {
        let lease = open_frames(handle).map_err(error)?;
        let mut direct = Self {
            lease,
            uploaded: None,
            presentations: Vec::new(),
            last_submission: None,
            gate: SeekGate::default(),
            configured: false,
        };
        if props.active {
            direct.apply(props)?;
        } else {
            direct.apply_if_owner(props)?;
        }
        direct.configure(device)?;
        Ok(direct)
    }

    pub fn configure(&mut self, device: Arc<dyn DecodeDevice>) -> Result<bool, VideoError> {
        self.reset_device();
        self.configured = self
            .lease
            .configure_decoder_if_owner(Some(device))
            .map_err(error)?;
        Ok(self.configured)
    }
    pub fn configured(&self) -> bool {
        self.configured
    }

    pub fn reject_decoder(&mut self, detail: &str) -> Result<bool, VideoError> {
        let owned = self.lease.reject_decoder_if_owner(detail).map_err(error)?;
        self.reset_device();
        Ok(owned)
    }

    pub fn info(&self) -> Result<Option<FrameInfo>, VideoError> {
        self.lease.info().map_err(error)
    }
    pub fn position(&self) -> Result<Option<PlaybackPosition>, VideoError> {
        self.lease.position().map_err(error)
    }
    pub fn holding(&self) -> Result<bool, VideoError> {
        self.lease.holding().map_err(error)
    }

    pub fn apply(&self, props: &VideoProps) -> Result<(), VideoError> {
        // The shared session gates PCM until this exact video generation is drawn.
        self.lease
            .control(PlaybackControl {
                playing: !props.paused && props.active,
                volume: props.volume,
                muted: props.muted,
                rate: props.rate,
            })
            .map_err(error)
    }

    pub fn apply_if_owner(&self, props: &VideoProps) -> Result<bool, VideoError> {
        self.lease
            .control_if_owner(PlaybackControl {
                playing: !props.paused && props.active,
                volume: props.volume,
                muted: props.muted,
                rate: props.rate,
            })
            .map_err(error)
    }

    pub fn pause_if_owner(&self) -> Result<(), VideoError> {
        self.lease.pause_if_owner().map(|_| ()).map_err(error)
    }
    pub fn is_owner(&self) -> Result<bool, VideoError> {
        self.lease.is_control_owner().map_err(error)
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<bool, VideoError> {
        self.reset_device();
        if let Some(generation) = self.lease.seek_if_owner(position_ms).map_err(error)? {
            self.gate.begin(generation);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn seek_complete(&mut self) -> bool {
        self.gate.take_complete()
    }

    pub fn empty_edit(&mut self) {
        self.uploaded = None;
        self.last_submission = None;
        // No media frame exists for an empty edit. Keep submitted GPU queries
        // alive for retirement; the seek no longer waits for a nonexistent frame.
        self.gate.empty_edit();
    }

    pub fn frame(&self, clock_secs: f64) -> Result<Option<Arc<PresentationFrame>>, VideoError> {
        self.lease
            .native_frame_at(clock_secs)
            .map(|frame| frame.filter(|frame| self.gate.accepts(frame.generation)))
            .map_err(error)
    }

    pub fn was_uploaded(&self, frame: &PresentationFrame) -> bool {
        self.uploaded
            .as_ref()
            .is_some_and(|old| (old.generation, old.serial) == (frame.generation, frame.serial))
    }
    pub fn uploaded(&mut self, frame: Arc<PresentationFrame>) {
        self.uploaded = Some(frame);
    }
    pub fn needs_raw(&self) -> Result<bool, VideoError> {
        self.holding()
    }

    pub fn reset_device(&mut self) {
        self.uploaded = None;
        self.presentations.clear();
        self.last_submission = None;
        self.gate.reset();
    }

    pub fn poll_presented(&mut self) -> Result<(), VideoError> {
        let mut remaining = Vec::new();
        for pending in self.presentations.drain(..) {
            if !pending.completed()? {
                remaining.push(pending);
                continue;
            }
            if self
                .lease
                .presented_if_owner(pending.generation, pending.serial)
                .map_err(error)?
            {
                self.gate.presented(pending.generation);
            }
        }
        self.presentations = remaining;
        Ok(())
    }

    /// Called only after this exact raw frame's DrawBitmap succeeds.
    pub fn submitted(
        &mut self,
        dc: &ID2D1DeviceContext,
        device: &ID3D11Device,
    ) -> Result<(), VideoError> {
        if let Some(frame) = &self.uploaded {
            self.submitted_frame(dc, device, frame.generation, frame.serial)?;
        }
        Ok(())
    }

    pub fn submitted_frame(
        &mut self,
        dc: &ID2D1DeviceContext,
        device: &ID3D11Device,
        generation: u64,
        serial: u64,
    ) -> Result<(), VideoError> {
        if (cfg!(feature = "decoder-audit") || self.needs_raw()?)
            && self.last_submission != Some((generation, serial))
        {
            if self.presentations.len() >= 64 {
                return Err(error(
                    "presentation feedback backlog exceeded its bounded audit queue",
                ));
            }
            self.presentations
                .push(Presentation::submit(dc, device, generation, serial)?);
            self.last_submission = Some((generation, serial));
        }
        Ok(())
    }
}

fn error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::DecoderFailed, error.to_string())
}
