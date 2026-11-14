use super::{bridge::Bridge, stats::Stats};
use hongguo_codec_ffi::{Backend, Codec, DecoderConfig, Packet, VideoDecoder, VideoFrame};
use hongguo_media_types::{
    DecodeDevice, FrameDescriptor, FrameImage, HardwareFrame, HardwareHandle, HardwareKind,
    PresentationFrame,
};
use hongguo_nsh_video_api::frames::{
    FrameInfo, FrameSource, PixelFrame, PlaybackControl, PlaybackPosition,
};
use hongguo_playback::{IndexedSource, MonotonicClock, TrackCodec};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows::{Win32::Graphics::Direct3D11::ID3D11Device, core::Interface};

const QUEUE_FRAMES: usize = 3;
struct State {
    queue: VecDeque<Arc<PresentationFrame>>,
    current: Option<Arc<PresentationFrame>>,
    clock: MonotonicClock,
    holding: bool,
    playing: bool,
    bound: bool,
    done: bool,
    duration: f64,
    error: Option<String>,
}
pub(super) struct Source {
    data: Arc<[u8]>,
    index: Arc<IndexedSource>,
    backend: String,
    limit: u64,
    state: Mutex<State>,
    changed: Condvar,
    cancel: AtomicBool,
    ended_seen: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
    pub stats: Arc<Stats>,
}
impl Source {
    pub fn new(
        data: Arc<[u8]>,
        index: IndexedSource,
        backend: String,
        limit: u64,
        stats: Arc<Stats>,
    ) -> Arc<Self> {
        let duration = index.duration_secs;
        Arc::new(Self {
            data,
            index: Arc::new(index),
            backend,
            limit,
            state: Mutex::new(State {
                queue: VecDeque::new(),
                current: None,
                clock: MonotonicClock::new(0.0, 1.0, Instant::now())
                    .expect("fixed clock configuration"),
                holding: true,
                playing: false,
                bound: false,
                done: false,
                duration,
                error: None,
            }),
            changed: Condvar::new(),
            cancel: AtomicBool::new(false),
            ended_seen: AtomicBool::new(false),
            worker: Mutex::new(None),
            stats,
        })
    }
    pub fn bind(self: &Arc<Self>, device: Arc<dyn DecodeDevice>) -> Result<(), String> {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.bound {
                return Err(
                    "drawing device changed during a controlled audit; discard this run".into(),
                );
            }
            state.bound = true;
        }
        if device.kind() != HardwareKind::D3d11 {
            return Err("audit requires NSH's actual D3D11 drawing device".into());
        }
        let pointer = device.device() as *mut std::ffi::c_void;
        let native = unsafe { ID3D11Device::from_raw_borrowed(&pointer) }
            .ok_or("null NSH D3D11 device")?
            .clone();
        let description = unsafe {
            native
                .cast::<windows::Win32::Graphics::Dxgi::IDXGIDevice>()
                .map_err(error)?
                .GetAdapter()
                .map_err(error)?
                .GetDesc()
        }
        .map_err(error)?;
        let name = &description.Description[..description
            .Description
            .iter()
            .position(|ch| *ch == 0)
            .unwrap_or(description.Description.len())];
        self.stats.device(serde_json::json!({"adapter":String::from_utf16_lossy(name),"vendor_id":description.VendorId,"device_id":description.DeviceId,"luid_high":description.AdapterLuid.HighPart,"luid_low":description.AdapterLuid.LowPart}));
        let source = self.clone();
        let worker = std::thread::Builder::new()
            .name("decoder-audit".into())
            .spawn(move || {
                if let Err(error) = source.decode(native) {
                    source.stats.fail(error.clone());
                    source
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .error = Some(error);
                }
            })
            .map_err(|error| error.to_string())?;
        *self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(worker);
        Ok(())
    }
    pub fn finish(&self) {
        if !self.ended_seen.load(Ordering::Acquire) {
            self.stats.fail(
                "audit ended before the video timeline completed; discard this partial run".into(),
            );
        }
        self.close();
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            && worker.join().is_err()
        {
            self.stats.fail("decode audit worker panicked".into());
        }
    }

    fn decode(&self, device: ID3D11Device) -> Result<(), String> {
        let track = &self.index.video;
        let mut bridge = if self.backend == "d3d12" {
            Some(Bridge::new(&device)?)
        } else {
            None
        };
        let backend = bridge.as_ref().map_or(
            Backend::D3d11 {
                device: device.as_raw() as usize,
            },
            |bridge| Backend::D3d12 {
                device: bridge.device(),
            },
        );
        let codec =
            match track.codec {
                TrackCodec::Hevc => Codec::Hevc,
                TrackCodec::H264 => Codec::H264,
                _ => return Err(
                    "A/B inputs must be actual H.264 or HEVC MP4; ByteVC2 has no D3D12 candidate"
                        .into(),
                ),
            };
        let mut decoder = unsafe {
            VideoDecoder::new(DecoderConfig {
                codec,
                backend,
                extradata: track.extradata.clone(),
                time_base: (1, track.info.media_timescale as i32),
                threads: 0,
            })
        }
        .map_err(error)?;
        let mut serial = 0;
        for (&(offset, length), time) in track.info.samples.iter().zip(&track.samples) {
            if self.cancel.load(Ordering::Acquire) || serial >= self.limit {
                break;
            }
            let start = usize::try_from(offset).map_err(error)?;
            let end = usize::try_from(offset.checked_add(length).ok_or("sample offset overflow")?)
                .map_err(error)?;
            let packet = Packet {
                data: self
                    .data
                    .get(start..end)
                    .ok_or("sample outside local input")?,
                pts: Some(time.pts),
                dts: Some(time.dts),
                duration: i64::from(time.duration),
            };
            while !decoder.send(&packet).map_err(error)? {
                if !self.receive(&mut decoder, &mut bridge, &mut serial)? {
                    return self.done();
                }
            }
            if !self.receive(&mut decoder, &mut bridge, &mut serial)? {
                return self.done();
            }
        }
        while !decoder.drain().map_err(error)? {
            if !self.receive(&mut decoder, &mut bridge, &mut serial)? {
                return self.done();
            }
        }
        self.receive(&mut decoder, &mut bridge, &mut serial)?;
        self.done()
    }

    fn receive(
        &self,
        decoder: &mut VideoDecoder,
        bridge: &mut Option<Bridge>,
        serial: &mut u64,
    ) -> Result<bool, String> {
        while let Some(frame) = decoder.receive().map_err(error)? {
            if self.cancel.load(Ordering::Acquire) || *serial >= self.limit {
                return Ok(false);
            }
            let frame = Arc::new(frame);
            let decoded_at = Instant::now();
            let mut descriptor = frame.descriptor();
            if !matches!(descriptor.bit_depth, 8 | 10) {
                return Err(
                    "A/B audit supports native 8/10-bit NV12/P010, not a downloaded fallback"
                        .into(),
                );
            }
            descriptor.rotation_degrees = self.index.video.rotation_degrees;
            let container = &self.index.video.color;
            descriptor.color.primaries = descriptor.color.primaries.or(container.primaries);
            descriptor.color.transfer = descriptor.color.transfer.or(container.transfer);
            descriptor.color.matrix = descriptor.color.matrix.or(container.matrix);
            descriptor.color.full_range = descriptor.color.full_range.or(container.full_range);
            descriptor.color.sample_aspect_ratio = descriptor
                .color
                .sample_aspect_ratio
                .or(container.sample_aspect_ratio);
            let media_pts = frame.pts().ok_or("decoded frame is missing PTS")?;
            let duration = self
                .index
                .video
                .duration_for_pts(media_pts)
                .map_err(error)?;
            let mappings = self.index.video.timeline.media_to_movie(media_pts);
            if mappings.is_empty() {
                continue;
            }
            if mappings.len() != 1 {
                return Err(
                    "controlled A/B fixture requires one continuous presentation edit".into(),
                );
            }
            let pts = mappings[0].movie_secs;
            if *serial == 0 && pts.abs() > duration {
                return Err("controlled audit fixture must begin at presentation zero".into());
            }
            self.stats
                .decoded(*serial + 1, pts, duration, &descriptor, decoded_at);
            let image: Arc<dyn HardwareFrame> = if let Some(bridge) = bridge {
                loop {
                    if let Some((image, copy)) = bridge.copy(frame.clone(), descriptor.clone())? {
                        self.stats.copy(copy);
                        break image;
                    }
                    if self.cancel.load(Ordering::Acquire) {
                        return Ok(false);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            } else {
                let (_, object, subresource) = frame
                    .native_handle()
                    .ok_or("D3D11 audit candidate returned a software frame")?;
                Arc::new(D3d11Frame {
                    _frame: frame.clone(),
                    descriptor: descriptor.clone(),
                    handle: HardwareHandle {
                        kind: HardwareKind::D3d11,
                        object,
                        subresource,
                    },
                })
            };
            *serial += 1;
            let presentation = Arc::new(PresentationFrame {
                generation: 1,
                serial: *serial,
                pts_secs: pts,
                duration_secs: duration,
                image: FrameImage::Hardware(image),
            });
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while state.queue.len() >= QUEUE_FRAMES && !self.cancel.load(Ordering::Acquire) {
                state = self
                    .changed
                    .wait_timeout(state, Duration::from_millis(50))
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            if self.cancel.load(Ordering::Acquire) {
                return Ok(false);
            }
            state.duration = pts + duration;
            state.queue.push_back(presentation);
            self.changed.notify_all();
        }
        Ok(true)
    }
    fn done(&self) -> Result<(), String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .done = true;
        self.changed.notify_all();
        Ok(())
    }
}

pub(super) struct Consumer(pub Arc<Source>);
impl FrameSource for Consumer {
    fn configure_decoder(&self, device: Option<Arc<dyn DecodeDevice>>) -> Result<(), String> {
        self.0
            .bind(device.ok_or("audit cannot fall back to software")?)
    }
    fn info(&self) -> Result<Option<FrameInfo>, String> {
        Ok(Some(FrameInfo {
            width: self.0.index.video.info.width,
            height: self.0.index.video.info.height,
        }))
    }
    fn frame_at(&self, _: u64) -> Result<Option<Arc<PixelFrame>>, String> {
        Ok(None)
    }
    fn native_frame_at(&self, position: f64) -> Result<Option<Arc<PresentationFrame>>, String> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        while state.queue.front().is_some_and(|frame| {
            state.holding && state.current.is_none()
                || !state.holding && frame.pts_secs <= position + 0.008
        }) {
            let next = state.queue.pop_front().expect("front checked");
            if let Some(old) = state.current.replace(next) {
                self.0.stats.drop_frame(old.serial);
            }
        }
        self.0.changed.notify_all();
        Ok(state.current.clone())
    }
    fn seek(&self, _: u64) -> Result<u64, String> {
        Err("fixed audit timeline does not accept seek commands".into())
    }
    fn presented(&self, generation: u64, serial: u64) -> Result<bool, String> {
        self.0.stats.draw_done(serial);
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.holding
            && state
                .current
                .as_ref()
                .is_some_and(|frame| frame.generation == generation && frame.serial == serial)
        {
            state.holding = false;
            let now = Instant::now();
            let playing = state.playing;
            state.clock.set_running(playing, now);
            self.0.stats.start_clock(now);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn holding(&self) -> bool {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .holding
    }
    fn control(&self, control: PlaybackControl) -> Result<(), String> {
        if control.rate != 1.0 {
            return Err("audit requires a fixed 1x rate".into());
        }
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.playing = control.playing;
        let running = state.playing && !state.holding;
        state.clock.set_running(running, Instant::now());
        Ok(())
    }
    fn position(&self) -> Result<Option<PlaybackPosition>, String> {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        let position = state.clock.position(Instant::now());
        let ended = state.done && state.queue.is_empty() && position >= state.duration;
        if ended {
            self.0.ended_seen.store(true, Ordering::Release);
        }
        Ok(Some(PlaybackPosition {
            position_secs: position,
            duration_secs: state.duration,
            ended,
            buffering: state.holding,
            video_gap: false,
        }))
    }
    fn close(&self) {
        self.0.close();
    }
}
impl Source {
    fn close(&self) {
        self.cancel.store(true, Ordering::Release);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.queue.clear();
        state.current = None;
        self.changed.notify_all();
    }
}
struct D3d11Frame {
    _frame: Arc<VideoFrame>,
    descriptor: FrameDescriptor,
    handle: HardwareHandle,
}
// SAFETY: the AVFrame retains the exact decoder texture/slice and its pool lease.
unsafe impl HardwareFrame for D3d11Frame {
    fn handle(&self) -> HardwareHandle {
        self.handle
    }
    fn descriptor(&self) -> FrameDescriptor {
        self.descriptor.clone()
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
