#![cfg(target_os = "windows")]
use hongguo_audio_api::{
    AudioError, PcmFormat, PcmSource,
    native::{Api, Callbacks, Format, NativeOutput, NativePosition},
};
use std::{ffi::c_void, sync::Arc};

unsafe extern "C" {
    fn hg_pcm_output_open(format: Format, callbacks: Callbacks, output: *mut *mut c_void) -> i32;
    fn hg_pcm_output_control(output: *mut c_void, operation: i32, epoch: u64) -> i32;
    fn hg_pcm_output_position(output: *mut c_void, position: *mut NativePosition) -> i32;
    fn hg_pcm_output_close(output: *mut c_void);
}
static API: Api = Api {
    open: hg_pcm_output_open,
    control: hg_pcm_output_control,
    position: hg_pcm_output_position,
    close: hg_pcm_output_close,
};

pub fn open(format: PcmFormat, source: Arc<dyn PcmSource>) -> Result<NativeOutput, AudioError> {
    // SAFETY: The WASAPI backend serializes COM/control operations on its dedicated thread.
    unsafe { NativeOutput::open(format, source, &API) }
}
