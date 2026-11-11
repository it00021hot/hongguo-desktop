//! 解码能力探测：回答「当前机器能不能硬编、有没有 ffmpeg、会选哪条路」——实际的分流执行在壳内 `service::transcode_service::pipeline`。

pub mod bytevc2;
pub mod playback;
pub mod probe;

pub use bytevc2::bytevc2_decode_available;
pub use probe::selected_backend;
