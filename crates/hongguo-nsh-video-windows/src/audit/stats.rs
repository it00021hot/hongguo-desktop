use nsh_platform_windows::NativePresentation;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Instant,
};

use super::measurements::Frame;
#[derive(Default)]
struct State {
    clock_start_ms: Option<f64>,
    frames: BTreeMap<u64, Frame>,
    copy_bytes: u64,
    copy_submit_us: u64,
    gpu_waits: u64,
    copy_timings: Vec<Arc<Mutex<Option<f64>>>>,
    presents: Vec<Value>,
    error: Option<String>,
    format: Option<Value>,
    device: Option<Value>,
    display: Option<Value>,
    enhancement_required: bool,
}
pub(super) struct Stats {
    start: Instant,
    state: Mutex<State>,
}
impl Stats {
    pub fn require_enhancement(&self, required: bool) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enhancement_required = required;
    }
    pub fn new(start: Instant) -> Arc<Self> {
        Arc::new(Self {
            start,
            state: Mutex::new(State::default()),
        })
    }
    pub fn decoded(
        &self,
        serial: u64,
        pts: f64,
        duration: f64,
        descriptor: &hongguo_media_types::FrameDescriptor,
        at: Instant,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.format = Some(
            json!({"bit_depth":descriptor.bit_depth,"coded_width":descriptor.width,"coded_height":descriptor.height}),
        );
        state.frames.insert(
            serial,
            Frame {
                pts,
                duration,
                decoded_ms: at.saturating_duration_since(self.start).as_secs_f64() * 1000.0,
                ..Default::default()
            },
        );
    }
    pub fn start_clock(&self, at: Instant) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .clock_start_ms
            .get_or_insert(at.saturating_duration_since(self.start).as_secs_f64() * 1000.0);
    }
    pub fn device(&self, value: Value) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .device = Some(value);
    }
    fn display(&self, value: Value) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.display.as_ref().is_some_and(|old| old != &value) {
            state.error = Some("display color state changed during audit; discard this run".into());
        }
        state.display = Some(value);
    }
    pub fn draw_done(&self, serial: u64) {
        if let Some(frame) = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .frames
            .get_mut(&serial)
        {
            frame
                .draw_done_ms
                .get_or_insert(self.start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    pub fn drop_frame(&self, serial: u64) {
        if let Some(frame) = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .frames
            .get_mut(&serial)
        {
            frame.skipped();
        }
    }
    pub fn copy(&self, sample: super::bridge::CopySample) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.copy_bytes += sample.bytes;
        state.copy_submit_us += sample.submit_us;
        state.gpu_waits += sample.gpu_waits;
        state.copy_timings.push(sample.timing);
    }
    pub fn fail(&self, error: String) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .error
            .get_or_insert(error);
    }
    fn present(&self, serial: u64, enhanced: bool, info: &NativePresentation) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let at = info
            .submitted_at
            .saturating_duration_since(self.start)
            .as_secs_f64()
            * 1000.0;
        if let Some(frame) = state.frames.get_mut(&serial) {
            frame.presented(at);
        }
        state.presents.push(json!({"serial":serial,"enhanced":enhanced,"submitted_ms":at,"present_count":info.present_count,"dxgi_statistics":info.statistics.map(|s|json!({"reported_present_count":s.PresentCount,"present_refresh_count":s.PresentRefreshCount,"sync_refresh_count":s.SyncRefreshCount,"sync_qpc_reference":s.SyncQPCTime}))}));
    }
    pub fn report(&self) -> Value {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frames=state.frames.iter().map(|(&serial,frame)| {
            let due=frame.due(state.clock_start_ms);
            let deadline=frame.deadline(state.clock_start_ms);
            json!({"serial":serial,"pts_secs":frame.pts,"duration_secs":frame.duration,"decoder_returned_surface_ms":frame.decoded_ms,"gpu_draw_completed_ms":frame.draw_done_ms,"surface_to_draw_completion_ms_including_decode_copy_sync_color":frame.draw_done_ms.map(|end|end-frame.decoded_ms),"canvas_present_submitted_ms":frame.present_ms,"due_ms":due,"deadline_ms":deadline,"missed_deadline":frame.missed_deadline(state.clock_start_ms),"dropped_before_present":frame.dropped})
        }).collect::<Vec<_>>();
        let steady = frames.iter().skip(30).collect::<Vec<_>>();
        let missed = steady
            .iter()
            .filter(|f| f["missed_deadline"] == true)
            .count();
        let dropped = steady
            .iter()
            .filter(|f| f["dropped_before_present"] == true)
            .count();
        let copy_times = state
            .copy_timings
            .iter()
            .map(|value| {
                *value
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            })
            .collect::<Vec<_>>();
        let gpu_copy_us = copy_times
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .map(|times| times.into_iter().sum::<f64>());
        let raw_steady_presents = state
            .presents
            .iter()
            .filter(|present| {
                present["serial"].as_u64().is_some_and(|serial| serial > 30)
                    && present["enhanced"] == false
            })
            .count();
        let quality_error=(state.enhancement_required&&raw_steady_presents>0).then_some("requested enhancement was absent from steady-state presentations; discard this quality-mismatched trial");
        json!({
            "device":state.device,"display":state.display,
            "extra_d3d12_gpu_copy_us":gpu_copy_us,"copy_timing_queries":copy_times,
            "successful_copy_timestamp_readback_bytes":copy_times.iter().flatten().count()*16,
            "os_display_drop_count":Value::Null,"error":state.error.as_deref().or(quality_error),"observed_format":state.format,
            "steady_raw_present_submissions":raw_steady_presents,
            "first_enhanced_present_submitted_ms":state.presents.iter().find(|p|p["enhanced"]==true).and_then(|p|p["submitted_ms"].as_f64()),
            "gpu_completion_timestamp_is_cpu_observation_upper_bound":true,
            "first_gpu_draw_completed_ms":state.frames.values().find_map(|f|f.draw_done_ms),
            "first_canvas_present_submitted_ms":state.frames.values().find_map(|f|f.present_ms),
            "warmup_frames":30,"steady_frames":steady.len(),"steady_missed_deadlines":missed,"steady_dropped_before_present":dropped,
            "extra_d3d12_copy_bytes":state.copy_bytes,"extra_d3d12_copy_submit_cpu_us":state.copy_submit_us,
            "extra_d3d12_gpu_queue_waits":state.gpu_waits,"media_pixel_cpu_download_bytes":0,
            "physical_power_watts":Value::Null,"photon_time_ms":Value::Null,"frames":frames,"present_submissions":state.presents
        })
    }
}

fn registry() -> &'static Mutex<HashMap<String, Weak<Stats>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Weak<Stats>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}
pub(super) fn register(id: &str, stats: &Arc<Stats>) {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.to_owned(), Arc::downgrade(stats));
}
pub(crate) fn wants(id: &str) -> bool {
    lookup(id).is_some()
}
pub(crate) fn submitted(
    id: &str,
    serial: u64,
    enhanced: bool,
    info: &NativePresentation,
    display: Option<nsh_platform_windows::OutputColorInfo>,
) {
    if let Some(stats) = lookup(id) {
        stats.present(serial, enhanced, info);
        if let Some(display) = display {
            stats.display(json!({"mode":format!("{:?}",display.mode),"fallback_reason":display.fallback_reason.map(|reason|format!("{reason:?}")),"hdr_enabled":display.hdr_enabled,"sdr_white_level_nits":display.sdr_white_level_nits,"peak_nits":display.luminance.peak_nits,"red_xy":display.chromaticities.red,"green_xy":display.chromaticities.green,"blue_xy":display.chromaticities.blue,"white_xy":display.chromaticities.white}));
        }
    }
}
fn lookup(id: &str) -> Option<Arc<Stats>> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
        .and_then(Weak::upgrade)
}
