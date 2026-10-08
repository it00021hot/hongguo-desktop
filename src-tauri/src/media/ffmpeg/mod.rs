//! ffmpeg 集成：探测与转码执行。

pub mod probe;
pub mod transcode;

pub use probe::{backend_info, ffmpeg_path, h264_encoder};
pub use transcode::{TranscodeRequest, transcode_with_ffmpeg};
