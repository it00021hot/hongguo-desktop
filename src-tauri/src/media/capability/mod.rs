//! 解码能力探测。
//!
//! 只回答「当前机器能不能硬解、有没有 ffmpeg」，不决定走哪条路——
//! 分流决策在 [`crate::service::transcode_service::pipeline`]。

pub mod probe;
