//! macOS VideoToolbox 编码后端（硬编优先，无硬编回落 Apple 内置软编会话）：MP4 样本（长度前缀 NAL，原样）─VTDecompressionSession→ NV12
//! ─(VTPixelTransferSession 缩放可选)→ VTCompressionSession → H.264 AVCC ─Annex-B 转换 + SPS/PPS 前插→ mux_h264 → MP4 out。
//! 链路与 ffmpeg 路径逐段对应，「解码 → YUV → 编码」整体换成 GPU 上的「解码 → NV12 CVPixelBuffer → 编码」，IOSurface 背板像素缓冲编解码之间零拷贝；
//! 复用公共件（MP4 解复用 / hvcC 参数集 / 音轨 AAC 直通 / faststart 封装 / 原子替换），产物与 ffmpeg 完全同构；时间戳逐帧来自源样本表（stts/ctts 展开）。
//! 探测只答两问：会话能不能建（生产闸门，软编会话也算数）、是不是硬件（「能建会话」不算硬件，编码器自己说走了硬件才算；256×256 下限与 ffmpeg 侧 nvenc 教训一致）。
//! 布局：[`ffi`] 手写 FFI、[`pixel`] Pb/CmSb 包装 + Transfer、[`probe`] 进程级缓存、[`decode`]/[`encode`] 会话、[`pipeline`] 转码编排。

mod decode;
mod encode;
mod ffi;
mod pipeline;
mod pixel;
mod probe;
mod yuv;

pub use pipeline::transcode_h264;
pub use probe::{clear_probe_cache, encoder_available, h264_hw_encoder_available};
pub use yuv::{YuvEncoder, with_encoder_thread};

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_media_platform_api::video::bitrate_for;

    #[test]
    fn bitrate_lands_in_the_crf23_band() {
        let b = bitrate_for(1920, 1080, 25.0);
        assert!((5_900_000..=6_600_000).contains(&b), "实际 {b}");
        assert_eq!(bitrate_for(1920, 1080, 240.0), 12_000_000);
        assert_eq!(bitrate_for(320, 180, 10.0), 800_000);
    }

    #[test]
    fn probe_never_panics_without_a_gpu() {
        let _ = h264_hw_encoder_available();
    }
}
