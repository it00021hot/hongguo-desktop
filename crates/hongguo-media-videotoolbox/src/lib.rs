#![cfg(any(target_os = "macos", target_os = "ios"))]

mod vt;

pub use vt::{
    YuvEncoder, clear_probe_cache, encoder_available, hevc_hw_encoder_available, transcode_hevc,
    with_encoder_thread,
};
