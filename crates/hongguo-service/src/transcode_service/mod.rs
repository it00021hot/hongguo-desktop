//! 转码服务：HEVC / ByteVC2 → H.264（缓存与流水线编排），只供「兼容格式合并」使用——播放不转码。

pub mod cache;
pub mod pipeline;
