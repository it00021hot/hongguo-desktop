//! Shared software-decoded pixels with a window-thread audio/presentation gate.
use crate::{VideoError, presentation::Presentation, seek_gate::SeekGate};
use hongguo_nsh_video_api::{
    ErrorCode,
    frames::{FrameInfo, FrameLease, PixelFrame, open_frames},
};
use std::sync::Arc;
use windows::Win32::Graphics::{Direct2D::ID2D1DeviceContext, Direct3D11::ID3D11Device};

pub(crate) struct Direct {
    lease: FrameLease,
    pub audio_ready: bool,
    gate: SeekGate,
    uploaded: Option<Arc<PixelFrame>>,
    presentation: Option<Presentation>,
}

impl Direct {
    pub fn open(handle: &str) -> Result<Self, VideoError> {
        Ok(Self {
            lease: open_frames(handle).map_err(error)?,
            audio_ready: false,
            gate: SeekGate::default(),
            uploaded: None,
            presentation: None,
        })
    }

    pub fn info(&self) -> Result<Option<FrameInfo>, VideoError> {
        self.lease.info().map_err(error)
    }

    pub fn hold(&self) -> Result<bool, VideoError> {
        Ok(!self.audio_ready || self.lease.holding().map_err(error)? || self.gate.waiting())
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), VideoError> {
        self.gate
            .begin(self.lease.seek(position_ms).map_err(error)?);
        self.uploaded = None;
        self.presentation = None;
        Ok(())
    }

    pub fn audio_seeked(&mut self) {
        self.gate.audio_completed();
    }

    pub fn seek_complete(&self) -> bool {
        self.gate.complete()
    }

    pub fn frame(&self, clock_ms: u64) -> Result<Option<Arc<PixelFrame>>, VideoError> {
        self.lease
            .frame_at(clock_ms)
            .map(|frame| frame.filter(|frame| self.gate.accepts(frame.generation)))
            .map_err(error)
    }

    pub fn was_uploaded(&self, frame: &PixelFrame) -> bool {
        self.uploaded
            .as_ref()
            .is_some_and(|old| old.generation == frame.generation && old.serial == frame.serial)
    }

    pub fn uploaded(&mut self, frame: Arc<PixelFrame>) {
        self.uploaded = Some(frame);
    }

    pub fn needs_raw(&self) -> Result<bool, VideoError> {
        Ok(self.lease.holding().map_err(error)? || self.gate.needs_video())
    }

    pub fn reset_device(&mut self) {
        self.uploaded = None;
        self.presentation = None;
    }

    pub fn poll_presented(&mut self) -> Result<(), VideoError> {
        if let Some(pending) = &self.presentation
            && pending.completed()?
        {
            let pending = self
                .presentation
                .take()
                .expect("completed presentation query");
            self.lease
                .presented(pending.generation, pending.serial)
                .map_err(error)?;
            self.gate.video_completed(pending.generation);
        }
        Ok(())
    }

    /// Called only after this exact raw frame's DrawBitmap has succeeded.
    pub fn submitted(
        &mut self,
        dc: &ID2D1DeviceContext,
        device: &ID3D11Device,
    ) -> Result<(), VideoError> {
        if self.presentation.is_none()
            && self.needs_raw()?
            && let Some(frame) = &self.uploaded
        {
            self.presentation = Some(Presentation::submit(
                dc,
                device,
                frame.generation,
                frame.serial,
            )?);
        }
        Ok(())
    }
}

fn error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::DecoderFailed, error.to_string())
}
