mod picture;

use crate::{
    DisplayedFrame, RawFrame, VideoError,
    conversion::Conversion,
    device::DeviceLease,
    direct::Direct,
    drawing_device,
    state::{RawSnapshot, Shared},
};
use hongguo_nsh_video_api::{ErrorCode, Seek, Source, VideoEvent, VideoProps};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use windows::Win32::Graphics::{
    Direct2D::{ID2D1Bitmap1, ID2D1DeviceContext},
    Direct3D11::ID3D11Device,
};

pub(crate) struct Controller {
    source: Option<Source>,
    direct: Option<Direct>,
    device: Option<ID3D11Device>,
    conversion: Option<Conversion>,
    raw: Option<RawFrame>,
    loaded: bool,
    notified_size: Option<(u32, u32)>,
    applied: Option<VideoProps>,
    pending: Option<(Seek, Instant)>,
    queued: Option<Seek>,
    bitmap: Option<(Arc<DisplayedFrame>, ID2D1Bitmap1)>,
    progress_at: Instant,
    buffering: Option<bool>,
    ended: bool,
    waiting_for_device: bool,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            source: None,
            direct: None,
            device: None,
            conversion: None,
            raw: None,
            loaded: false,
            notified_size: None,
            applied: None,
            pending: None,
            queued: None,
            bitmap: None,
            progress_at: Instant::now(),
            buffering: None,
            ended: false,
            waiting_for_device: false,
        }
    }
}

impl Controller {
    pub fn set_props(&mut self, props: &VideoProps) -> Result<(), VideoError> {
        if self.device_removed() {
            self.suspend_for_device();
        }
        if !self.waiting_for_device
            && self.source == props.source
            && let Some(direct) = &mut self.direct
        {
            let rate_changed = self
                .applied
                .as_ref()
                .is_some_and(|old| old.rate != props.rate);
            let owned = if props.active {
                direct.apply(props)?;
                true
            } else {
                direct.apply_if_owner(props)?
            };
            if rate_changed && owned {
                direct.reset_device();
                self.raw = None;
                if let Some((request, _)) = self.pending.take()
                    && direct.seek(request.position_ms)?
                {
                    self.pending = Some((request, Instant::now()));
                }
            }
            if owned {
                self.applied = Some(props.clone());
            } else {
                self.applied = None;
                self.pending = None;
                self.queued = None;
            }
        }
        if self.queued.as_ref().is_some_and(|seek| {
            props
                .source
                .as_ref()
                .is_none_or(|source| source.id != seek.source_id)
        }) {
            self.queued = None;
        }
        self.bitmap = None;
        Ok(())
    }

    pub fn seek(&mut self, request: Seek, props: &VideoProps) -> Result<(), VideoError> {
        if request.source_id.is_empty() || request.request_id.is_empty() {
            return Err(VideoError::new(
                ErrorCode::InvalidProps,
                "seek requires source and request identities",
            ));
        }
        if props
            .source
            .as_ref()
            .is_none_or(|source| source.id != request.source_id)
        {
            return Ok(());
        }
        if !props.active {
            return Ok(());
        }
        self.bitmap = None;
        self.raw = None;
        if self.waiting_for_device
            || self.pending.is_some()
            || !self.loaded
            || self.source != props.source
        {
            self.queued = Some(request);
        } else if let Some(direct) = &mut self.direct {
            direct.apply(props)?;
            if direct.seek(request.position_ms)? {
                self.pending = Some((request, Instant::now()));
            }
            self.ended = false;
        } else {
            self.queued = Some(request);
        }
        Ok(())
    }

    pub fn draw(
        &mut self,
        shared: &Arc<Shared>,
        dc: &ID2D1DeviceContext,
        size: (f32, f32),
    ) -> Result<(), VideoError> {
        let props = shared
            .props
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if !shared
            .consumer_active
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        let device = match drawing_device(dc) {
            Ok(device) => device,
            Err(_) if self.device_removed() => {
                self.suspend_for_device();
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if unsafe { device.GetDeviceRemovedReason() }.is_err() {
            self.suspend_for_device();
            return Ok(());
        }
        let device_changed = self.device.as_ref() != Some(&device);
        if device_changed {
            self.waiting_for_device = false;
            self.conversion = Some(Conversion::new(&device)?);
            self.device = Some(device.clone());
            self.raw = None;
            self.bitmap = None;
            self.applied = None;
            shared.clear_frames();
        }
        if self.source != props.source {
            self.direct = None;
            self.raw = None;
            self.bitmap = None;
            self.applied = None;
            self.loaded = false;
            self.notified_size = None;
            self.pending = None;
            self.buffering = None;
            self.ended = false;
            shared.clear_frames();
            self.source = props.source.clone();
            let Some(source) = &self.source else {
                return Ok(());
            };
            shared.emit(VideoEvent::LoadStart {
                source_id: source.id.clone(),
            });
            let handle = source.frame_source.as_deref().ok_or_else(|| {
                VideoError::new(
                    ErrorCode::InvalidProps,
                    "builtin native frame source is required",
                )
            })?;
            self.direct = Some(Direct::open(
                handle,
                DeviceLease::new(
                    &device,
                    shared
                        .context
                        .drawing_device_generation()
                        .map_err(frame_error)?,
                ),
                &props,
            )?);
            self.applied = Some(props.clone());
        } else if let Some(direct) = &mut self.direct
            && (device_changed || !direct.configured())
        {
            let owned = direct.configure(DeviceLease::new(
                &device,
                shared
                    .context
                    .drawing_device_generation()
                    .map_err(frame_error)?,
            ))?;
            if owned && let Some((request, _)) = self.pending.take() {
                self.queued = Some(request);
            } else if !owned {
                self.pending = None;
                self.queued = None;
            }
        }
        let Some(mut direct) = self.direct.take() else {
            return Ok(());
        };
        let result = self.draw_source(shared, &mut direct, &props, dc, size);
        if result.is_err() && self.device_removed() {
            self.direct = Some(direct);
            self.suspend_for_device();
            return Ok(());
        }
        self.direct = Some(direct);
        result
    }

    fn device_removed(&self) -> bool {
        self.device
            .as_ref()
            .is_some_and(|device| unsafe { device.GetDeviceRemovedReason() }.is_err())
    }

    fn suspend_for_device(&mut self) {
        // NSH owns recreation of this device. Keep the registered consumer so
        // the next draw can bind the replacement; software upload cannot repair
        // a removed drawing device. Resume only after the new epoch is visible.
        if !self.waiting_for_device
            && let Some(direct) = &self.direct
            && let Err(error) = direct.pause_if_owner()
        {
            log::debug!("device-loss pause: {error}");
        }
        self.waiting_for_device = true;
        self.applied = None;
    }

    fn draw_source(
        &mut self,
        shared: &Arc<Shared>,
        direct: &mut Direct,
        props: &VideoProps,
        dc: &ID2D1DeviceContext,
        size: (f32, f32),
    ) -> Result<(), VideoError> {
        let source_id = self.source.as_ref().expect("opened source").id.clone();
        if !direct.is_owner()? {
            shared
                .consumer_active
                .store(false, std::sync::atomic::Ordering::Release);
            shared.clear_frames();
            self.raw = None;
            self.bitmap = None;
            self.pending = None;
            self.queued = None;
            return Ok(());
        }
        shared
            .consumer_active
            .store(true, std::sync::atomic::Ordering::Release);
        direct.poll_presented()?;
        if self.applied.as_ref().is_none_or(|old| {
            old.paused != props.paused
                || old.active != props.active
                || old.rate != props.rate
                || old.muted != props.muted
                || old.volume != props.volume
        }) {
            let owns = direct.apply_if_owner(props)?;
            if owns {
                self.applied = Some(props.clone());
            } else {
                self.pending = None;
                self.queued = None;
            }
        }
        let mut position = direct.position()?;
        if position.is_some_and(|position| position.video_gap) {
            direct.empty_edit();
        }
        if !self.loaded
            && let Some(info) = direct.info()?
        {
            self.loaded = true;
            if info.width == 0 || info.height == 0 {
                return Err(frame_error("invalid indexed video dimensions"));
            }
        }
        if self.pending.is_some() && direct.seek_complete() {
            let (completed, _) = self.pending.take().expect("completed seek");
            shared.emit(VideoEvent::Seeked {
                source_id: source_id.clone(),
                request_id: completed.request_id,
                position_ms: position.map_or(completed.position_ms, |p| millis(p.position_secs)),
            });
        }
        if self.loaded
            && self.pending.is_none()
            && let Some(request) = self.queued.take()
            && direct.seek(request.position_ms)?
        {
            self.raw = None;
            self.bitmap = None;
            shared.clear_frames();
            self.pending = Some((request, Instant::now()));
            self.ended = false;
            position = direct.position()?;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, started)| started.elapsed() >= Duration::from_secs(15))
        {
            return Err(VideoError::new(
                ErrorCode::SeekFailed,
                "native seek presentation timed out",
            ));
        }
        let holding = direct.holding()?;
        shared
            .awaiting_presentation
            .store(holding, std::sync::atomic::Ordering::Release);
        let buffering = holding || position.is_some_and(|position| position.buffering);
        if self.buffering != Some(buffering) {
            self.buffering = Some(buffering);
            shared.emit(VideoEvent::Buffer {
                source_id: source_id.clone(),
                is_buffering: buffering,
            });
        }
        let gap = position.is_some_and(|position| position.video_gap);
        shared.set_video_gap(gap);
        if gap {
            direct.empty_edit();
            self.raw = None;
            self.bitmap = None;
        }
        if props.active
            && !gap
            && let Some(frame) = direct.frame(position.map_or(0.0, |p| p.position_secs))?
        {
            let display = shared.context.output_color_info().map_err(frame_error)?;
            let conversion = self.conversion.as_mut().expect("drawing conversion");
            let enhance = props.enhancement.anime4k_on || props.enhancement.sharpen_on;
            if !direct.was_uploaded(&frame)
                || conversion.display_changed(&frame.image, display)
                || self
                    .raw
                    .as_ref()
                    .is_none_or(|raw| raw.has_working() != enhance)
            {
                if conversion.display_changed(&frame.image, display) {
                    shared.invalidate();
                }
                let dimensions = crate::draw::frame_dimensions(&frame.image.descriptor())?;
                if self.raw.as_ref().is_none_or(|raw| raw.size() != dimensions) {
                    self.raw = Some(RawFrame::new(dc, dimensions.0, dimensions.1, display.mode)?);
                }
                let mut snapshot = shared
                    .raw
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let raw = self.raw.as_mut().expect("native raw frame initialized");
                let retirement = crate::presentation::Presentation::prepare(
                    self.device.as_ref().expect("drawing device"),
                    frame.generation,
                    frame.serial,
                )?;
                if let Err(error) = raw.convert(dc, conversion, &frame, display, enhance) {
                    if error.code() == ErrorCode::FrameFormatUnsupported
                        && matches!(&frame.image, hongguo_media_types::FrameImage::Hardware(_))
                    {
                        // Device loss needs NSH device recreation. A working device with
                        // an unusable decoder surface rejects only that core candidate.
                        unsafe {
                            self.device
                                .as_ref()
                                .expect("drawing device")
                                .GetDeviceRemovedReason()
                        }
                        .map_err(frame_error)?;
                        drop(snapshot);
                        let owned = direct.reject_decoder(error.detail())?;
                        self.raw = None;
                        self.bitmap = None;
                        shared.clear_frames();
                        if owned && let Some((request, _)) = self.pending.take() {
                            self.queued = Some(request);
                        } else if !owned {
                            self.pending = None;
                            self.queued = None;
                        }
                        return Ok(());
                    }
                    return Err(error);
                }
                shared.retain(retirement, frame.clone());
                *snapshot = raw.metadata().map(|metadata| RawSnapshot {
                    device: self.device.as_ref().expect("drawing device").clone(),
                    texture: raw.texture().clone(),
                    metadata,
                    color: conversion.transform(),
                    output_mode: display.mode,
                });
                shared.wake_gpu.notify_one();
                direct.uploaded(frame);
            }
        }
        if let Some(metadata) = self.raw.as_ref().and_then(RawFrame::metadata) {
            let (numerator, denominator) = metadata.sample_aspect_ratio;
            if numerator == 0 || denominator == 0 {
                return Err(frame_error("invalid pixel aspect ratio"));
            }
            let width = (u64::from(metadata.width) * u64::from(numerator)
                + u64::from(denominator) / 2)
                / u64::from(denominator);
            let width = u32::try_from(width).map_err(frame_error)?;
            if width == 0 || width > i32::MAX as u32 {
                return Err(frame_error("invalid displayed video dimensions"));
            }
            if self.notified_size != Some((width, metadata.height)) {
                self.notified_size = Some((width, metadata.height));
                shared.emit(VideoEvent::Load {
                    source_id: source_id.clone(),
                    duration_ms: position.map(|p| millis(p.duration_secs)),
                    width,
                    height: metadata.height,
                });
            }
        }
        if self.progress_at.elapsed()
            >= Duration::from_millis(u64::from(props.progress_interval_ms))
            && let Some(position) = position
        {
            self.progress_at = Instant::now();
            shared.emit(VideoEvent::Progress {
                source_id: source_id.clone(),
                position_ms: millis(position.position_secs),
                duration_ms: Some(millis(position.duration_secs)),
            });
            if position.ended && !self.ended {
                self.ended = true;
                shared.emit(VideoEvent::End { source_id });
            }
        }
        // Keep the consumer available to draw_picture's visible presentation gate.
        if gap {
            Ok(())
        } else {
            self.draw_picture_with(shared, props, direct, dc, size)
        }
    }

    pub fn stop(&mut self) {
        if let Some(direct) = &self.direct {
            let _ = direct.pause_if_owner();
        }
        self.direct = None;
        self.pending = None;
        self.queued = None;
        self.raw = None;
        self.bitmap = None;
    }
}

fn millis(seconds: f64) -> u64 {
    (seconds.max(0.0) * 1000.0).round() as u64
}
fn frame_error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::FrameUnavailable, error.to_string())
}
