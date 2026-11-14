use std::ffi::{c_char, c_double, c_int, c_void};

pub const NO_TIMESTAMP: i64 = i64::MIN;

#[derive(Default)]
#[repr(C)]
pub struct FrameInfo {
    pub width: c_int,
    pub height: c_int,
    pub bit_depth: c_int,
    pub chroma_w: c_int,
    pub chroma_h: c_int,
    pub planes: c_int,
    pub crop_left: c_int,
    pub crop_top: c_int,
    pub crop_right: c_int,
    pub crop_bottom: c_int,
    pub primaries: c_int,
    pub transfer: c_int,
    pub matrix: c_int,
    pub full_range: c_int,
    pub chroma_location: c_int,
    pub sar_num: c_int,
    pub sar_den: c_int,
    pub backend: c_int,
    pub pts: i64,
    pub duration: i64,
    pub handle: usize,
    pub subresource: usize,
    pub data: [*const u8; 4],
    pub stride: [c_int; 4],
    pub plane_width: [c_int; 4],
    pub plane_height: [c_int; 4],
    pub bytes_per_sample: c_int,
    pub sample_step: [c_int; 4],
    pub sample_shift: [c_int; 4],
    pub big_endian: c_int,
    pub has_mastering: c_int,
    pub has_content_light: c_int,
    pub mastering_primaries: [u16; 6],
    pub white_point: [u16; 2],
    pub max_luminance: u32,
    pub min_luminance: u32,
    pub max_cll: u16,
    pub max_fall: u16,
}

#[derive(Default)]
#[repr(C)]
pub struct PcmInfo {
    pub samples: *const f32,
    pub frames: c_int,
    pub channels: c_int,
    pub sample_rate: c_int,
    pub pts: i64,
    pub media_start_secs: c_double,
    pub media_duration_secs: c_double,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct D3d12Frame {
    pub texture: usize,
    pub subresource: u32,
    pub fence: usize,
    pub fence_value: u64,
}

unsafe extern "C" {
    #[cfg(test)]
    pub fn hg_mediacodec_timestamp_key(pts: i64, time_num: c_int, time_den: c_int) -> i64;
    pub fn hg_video_open(
        out: *mut *mut c_void,
        codec: c_int,
        backend: c_int,
        device: usize,
        surface: usize,
        extra: *const u8,
        extra_size: c_int,
        time_num: c_int,
        time_den: c_int,
        threads: c_int,
    ) -> c_int;
    pub fn hg_video_free(video: *mut *mut c_void);
    pub fn hg_video_send(
        video: *mut c_void,
        data: *const u8,
        size: c_int,
        pts: i64,
        dts: i64,
        duration: i64,
    ) -> c_int;
    pub fn hg_video_receive(video: *mut c_void, frame: *mut *mut c_void) -> c_int;
    pub fn hg_video_flush(video: *mut c_void);
    pub fn hg_frame_info(frame: *const c_void, info: *mut FrameInfo) -> c_int;
    pub fn hg_frame_free(frame: *mut *mut c_void);
    pub fn hg_frame_download(frame: *const c_void, out: *mut *mut c_void) -> c_int;
    pub fn hg_frame_render(frame: *mut c_void, time_ns: i64) -> c_int;
    pub fn hg_frame_d3d12(frame: *const c_void, info: *mut D3d12Frame) -> c_int;
    pub fn hg_error_string(code: c_int, buffer: *mut c_char, size: usize) -> c_int;
    pub fn hg_audio_open(
        out: *mut *mut c_void,
        extra: *const u8,
        extra_size: c_int,
        input_rate: c_int,
        output_rate: c_int,
        channels: c_int,
        rate: c_double,
    ) -> c_int;
    pub fn hg_audio_free(audio: *mut *mut c_void);
    pub fn hg_audio_send(audio: *mut c_void, data: *const u8, size: c_int, pts: i64) -> c_int;
    pub fn hg_audio_receive(audio: *mut c_void, info: *mut PcmInfo) -> c_int;
    pub fn hg_audio_reset(audio: *mut c_void, rate: c_double) -> c_int;
}
