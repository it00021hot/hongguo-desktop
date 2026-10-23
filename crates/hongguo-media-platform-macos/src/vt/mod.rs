//! macOS VideoToolbox 编码后端（硬编优先，无硬编回落 Apple 内置软编会话）。
//!
//! 链路与软解路径（`hongguo_media::transcode`）逐段对应，只是「解码 → YUV → 编码」
//! 被整体换成了 GPU 上的「解码 → NV12 CVPixelBuffer → 编码」，IOSurface
//! 背板的像素缓冲在编解码之间零拷贝：
//!
//! ```text
//! MP4 样本（长度前缀 NAL，原样）─VTDecompressionSession→ NV12
//!     ─(VTPixelTransferSession 缩放，可选)→ VTCompressionSession
//!     → H.264 AVCC ─Annex-B 转换 + SPS/PPS 前插→ mux_h264 → MP4 out
//! ```
//!
//! 复用软解路径的部分：MP4 解复用、参数集读取（`hvcC`）、音轨 AAC 直通、
//! 封装（muxide faststart）、临时文件原子替换。产物与软解/ffmpeg 完全同构。
//!
//! 时间戳：逐帧显示时间来自源样本表（`stts`/`ctts` 展开），不再按固定帧率
//! 合成——换后端顺手消灭第一类音画漂移。
//!
//! 探测只回答两个问题：会话能不能建（生产闸门——软编会话也算数）、
//! 是不是硬件（能力徽标与并行度）。纪律与 ffmpeg 侧一致：「能建会话」
//! 不算硬件，编码器自己说「我走了硬件」才算；256×256 的下限与 ffmpeg 侧
//! nvenc 的教训一致（更小的探测帧会被最小分辨率拒掉，把可用的硬编误判成
//! 不可用）。
//!
//! 模块布局：
//!
//! - [`ffi`]：VideoToolbox / CoreFoundation / CoreMedia 手写 FFI
//! - [`pixel`]：Pb/CmSb 裸指针包装、CF 小工具、像素搬移会话（Transfer）。
//!   VT 回调 trampoline 的 refcon 指向会话内部队列，故 decode_cb/encode_cb
//!   分别与 Decompressor/Compressor 同文件（decode/encode），不在此处
//! - [`probe`]：能力探测（会话可建吗 / 是不是硬件），进程级缓存
//! - [`decode`]：HEVC / H.264 解码会话（转码链的解码步）
//! - [`encode`]：H.264 编码会话
//! - [`pipeline`]：HEVC→H.264 转码编排

mod decode;
mod encode;
mod ffi;
mod pipeline;
mod pixel;
mod probe;

pub use pipeline::transcode_h264;
pub use probe::{clear_probe_cache, encoder_available, h264_hw_encoder_available};

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_media_platform_api::video::bitrate_for;

    #[test]
    fn bitrate_lands_in_the_crf23_band() {
        // 1080p25 → 6.22 Mbps：与 libx264 crf23 的产物同档（0.12 系数）
        let b = bitrate_for(1920, 1080, 25.0);
        assert!((5_900_000..=6_600_000).contains(&b), "实际 {b}");
        // 夹在合理区间
        assert_eq!(bitrate_for(1920, 1080, 240.0), 12_000_000);
        assert_eq!(bitrate_for(320, 180, 10.0), 800_000);
    }

    #[test]
    fn probe_never_panics_without_a_gpu() {
        // 无 GPU/被禁用的机器上安静地返回
        let _ = h264_hw_encoder_available();
    }
}
