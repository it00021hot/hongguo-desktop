//! Windows Media Foundation 硬编后端（HEVC 解码 → NV12 → H.264 硬编）。
//!
//! 链路与 macOS 侧（[`super::vt`]）/软解路径逐段对应，只是「解码 → YUV →
//! 编码」被整体换成 Media Foundation 的 MFT 流水线，帧以 **NV12 系统内存**
//! 在 MFT 之间搬运（v1 形态：不建 D3D11 设备管理器——实测 NVIDIA 编码 MFT
//! 注册为 `d3d11_aware=false`，系统内存直进直出即可工作）：
//!
//! ```text
//! MP4 样本（长度前缀 NAL，原样）─HEVC 解码 MFT→ NV12
//!     ─(Video Processor MFT 缩放，可选)→ H.264 编码 MFT（硬件优先）
//!     → H.264 AVCC ─Annex-B 转换 + SPS/PPS 前插→ mux_h264 → MP4 out
//! ```
//!
//! MFT 有两种驱动契约，都实现了：**同步**（`ProcessInput`/`ProcessOutput`
//! 推拉，in-box 软件 MFT）与**异步**（事件循环 `METransformNeedInput`/
//! `METransformHaveOutput`/`METransformDrainComplete`，硬件 MFT 的契约，
//! 使用前须在 MFT 属性表上解锁 `MF_TRANSFORM_ASYNC_UNLOCK`）。解码取
//! 枚举排序第一（有硬解 MFT 的机器自动吃硬解，本机实况是微软 HEVC 扩展的
//! 软件解码器）；编码硬件优先，测试通道（`transcode_h264_for_tests`）
//! 回落 in-box 软件编码器 h264_mf——两条驱动路径因此都被 CI 覆盖。
//!
//! 复用软解路径的部分与 vt 侧相同：MP4 解复用、音轨 AAC 直通、封装
//! （muxide faststart）、临时文件原子替换；Annex-B 转换同一份
//! `hongguo_media::hevc::to_annexb`。时间戳同样读**输出样本自带的 PTS**（100ns），
//! 不依赖「输出序=输入序」——个别编码器拒收「关 B 帧」时可能重排。
//!
//! COM/线程模型：整个会话（探测与转码）跑在 `std::thread::scope` 的专用
//! 线程上，进入时 `CoInitializeEx(MTA)` + `MFStartup`，退出对应清理——
//! COM 状态不污染 tokio 工作线程。
//!
//! 探测纪律与 ffmpeg/vt 侧同一套：`MFT_ENUM_FLAG_HARDWARE` 枚举到 ≠ 能用
//! （本机枚举出的 3 个「硬件」编码器里就有注册信息残缺的），必须真实建
//! 会话**实编一帧 256×256** 并拿到输出样本才算数；同时要求 HEVC 解码 MFT
//! 在位（缺解码器的平台层没有意义）。
//!
//! 模块布局（形态照 [`super::vt`]）：
//!
//! - [`probe`]：MF_VERSION 与探测——硬编实编一帧的缓存结论、HEVC 解码
//!   MFT 在位（hevc_decoder_present）
//! - [`mft`]：会话线程（COM/MTA + MFStartup 一次初始化）、MFT 枚举/激活、
//!   同步/异步两套驱动与统一 Driver
//! - [`sample`]：媒体类型与样本小工具（locked_buffer/finish_sample 建样本骨架）
//! - [`decoder`]：SourceReader HEVC 解码 → NV12 系统内存
//! - [`scaler`]：Video Processor MFT 缩放
//! - [`encoder`]：H.264 编码 MFT（硬件优先 / 测试回落软件）
//! - [`annexb`]：参数集提取与 Annex-B/AVCC 切分
//! - [`pipeline`]：HEVC→H.264 转码编排（transcode_h264 入口）

// windows-rs 的事件类型常量（METransformNeedInput 等）沿用 MF 的混合大小写
// 命名，出现在 match 模式里会触发 non_upper_case_globals。
#![allow(non_upper_case_globals)]

mod annexb;
mod decoder;
mod encoder;
mod mft;
mod pipeline;
mod probe;
mod sample;
mod scaler;

pub use pipeline::transcode_h264;
#[cfg(any(test, feature = "test-support"))]
pub use pipeline::transcode_h264_for_tests;
pub use probe::{clear_probe_cache, h264_hw_encoder_available};
pub use probe::hevc_decoder_present;

// 下方 tests 原样保留，引用的跨子模块名字由此供给。
#[cfg(test)]
use annexb::param_sets_annexb;
#[cfg(test)]
use hongguo_media_platform_api::PlatformRequest;
#[cfg(test)]
use hongguo_media_platform_api::video::bitrate_for;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_lands_in_the_crf23_band() {
        // 与 vt 侧同档：1080p25 → 6.22 Mbps（0.12 系数，见 bitrate_for 注释）
        let b = bitrate_for(1920, 1080, 25.0);
        assert!((5_900_000..=6_600_000).contains(&b), "实际 {b}");
        assert_eq!(bitrate_for(1920, 1080, 240.0), 12_000_000);
        assert_eq!(bitrate_for(320, 180, 10.0), 800_000);
    }

    /// 同进程连续两次平台转码：第二次不能劣化（应用里用户连转多集）。
    #[test]
    fn consecutive_sessions_in_one_process() {
        let Some(dir) = std::env::var_os("HONGGUO_E2E_DIR") else {
            return;
        };
        let src = std::path::Path::new(&dir).join("001.mp4");
        let out1 = std::env::temp_dir().join("hg-mf-twice-1.mp4");
        let run = |out: &std::path::Path| {
            transcode_h264_for_tests(&PlatformRequest {
                input: &src,
                output: out,
                scale_to: None,
                on_progress: None,
            })
            .expect("应可转码")
            .expect("应成功");
            let d = hongguo_media::demux::demux_file(out).unwrap();
            let n = d.video_track().unwrap().info.samples.len();
            let _ = std::fs::remove_file(out);
            n
        };
        let n1 = run(&out1);
        // 第二次走硬编入口（应用里更接近「探测后真转码」的路径）
        let out3 = std::env::temp_dir().join("hg-mf-twice-3.mp4");
        transcode_h264(&PlatformRequest {
            input: &src,
            output: &out3,
            scale_to: None,
            on_progress: None,
        })
        .expect("硬编应可用")
        .expect("应成功");
        let d = hongguo_media::demux::demux_file(&out3).unwrap();
        let n3 = d.video_track().unwrap().info.samples.len();
        let _ = std::fs::remove_file(&out3);
        eprintln!("[dbg] 软件 {n1} 帧 → 硬编 {n3} 帧");
        assert_eq!(n1, n3, "硬编会话在软编之后劣化了");
    }

    #[test]
    fn probe_never_panics_without_a_gpu() {
        // 无 GPU/被禁用的机器上安静地返回
        let _ = h264_hw_encoder_available();
    }

    #[test]
    fn param_sets_annexb_parses_both_shapes() {
        // Annex-B 形态：SPS(7) + IDR(5) + PPS(8) → 只留 7/8
        let annexb = [
            &[0u8, 0, 0, 1][..],
            &[0x67u8, 0x00, 0x00, 0x00], // SPS
            &[0, 0, 0, 1],
            &[0x65, 0xAA], // IDR，剔除
            &[0, 0, 0, 1],
            &[0x68, 0xBB], // PPS
        ]
        .concat();
        let out = param_sets_annexb(&annexb);
        assert_eq!(
            out,
            [
                [0, 0, 0, 1].to_vec(),
                vec![0x67, 0x00, 0x00, 0x00],
                [0, 0, 0, 1].to_vec(),
                vec![0x68, 0xBB],
            ]
            .concat()
        );

        // AVCC 形态：4 字节长度前缀
        let avcc = [
            &(2u32).to_be_bytes()[..],
            &[0x67u8, 0x11], // SPS
            &(2u32).to_be_bytes(),
            &[0x65, 0x22], // IDR，剔除
        ]
        .concat();
        let out = param_sets_annexb(&avcc);
        assert_eq!(out, [[0, 0, 0, 1].to_vec(), vec![0x67, 0x11]].concat());
    }
}
