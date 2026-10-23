//! 视频解码出口：契约与数据在 api crate，具体类型按编译目标 re-export。
//!
//! 引擎（hongguo-playback）与壳（hongguo-gpui）只认这一层：
//! macOS 实现走 VideoToolbox（NV12 CVPixelBuffer，渲染零拷贝），Windows
//! 实现走 Media Foundation（NV12 系统内存帧）。

pub use hongguo_media_platform_api::video::{
    DecoderInit, FrameBuffer, VideoCodec, VideoDecoder, VideoFrame,
};

#[cfg(target_os = "windows")]
pub use hongguo_media_platform_api::video::Nv12Plane;

#[cfg(target_os = "macos")]
pub use hongguo_media_platform_macos::video::VideoFrameDecoder;
#[cfg(target_os = "windows")]
pub use hongguo_media_platform_windows::video::VideoFrameDecoder;
