//! 转码服务：HEVC → H.264（缓存与流水线编排），供「兼容格式合并」与播放兼容兜底共用。

pub mod cache;
pub mod pipeline;
