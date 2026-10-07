//! 解码能力探测。
//!
//! 回答「当前机器能不能硬编、有没有 ffmpeg、会选哪条路」——实际的
//! 分流执行在 [`crate::service::transcode_service::pipeline`]。

pub mod probe;

pub use probe::{scaling_available, selected_backend};
