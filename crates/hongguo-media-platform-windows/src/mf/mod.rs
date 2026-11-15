//! HEVC 编码与源 HEVC 解码均在独立 COM/MF 会话线程上执行。

#![expect(
    non_upper_case_globals,
    reason = "Media Foundation SDK 事件常量保留官方命名"
)]

mod annexb;
mod decoder;
mod encoder;
mod mft;
mod pipeline;
mod probe;
mod sample;
mod scaler;
mod yuv_encode;

pub use pipeline::transcode_hevc;
pub use probe::{clear_probe_cache, hevc_hw_encoder_available, hevc_sw_encoder_available};
pub use yuv_encode::{YuvEncoder, with_encoder_thread};
