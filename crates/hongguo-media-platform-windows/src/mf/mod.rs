//! HEVC 编码与源 HEVC 解码均在独立 COM/MF 会话线程上执行。

#![expect(
    non_upper_case_globals,
    reason = "Media Foundation SDK 事件常量保留官方命名"
)]

mod annexb;
mod decoder;
mod encoder;
mod mft;
mod pipeline;
mod probe;
mod runtime;
mod sample;
mod scaler;
mod symbols;
mod yuv_encode;

pub use pipeline::transcode_hevc;
#[cfg(any(test, feature = "test-support"))]
pub use pipeline::transcode_hevc_for_tests;
pub use probe::{clear_probe_cache, hevc_hw_encoder_available, hevc_sw_encoder_available};
pub use yuv_encode::{YuvEncoder, with_encoder_thread};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_never_panics_without_a_gpu() {
        let _ = hevc_hw_encoder_available();
    }

    #[test]
    #[ignore = "需要支持 HEVC 硬编的 Windows 真机"]
    fn hardware_probe_requires_a_real_encoded_sample() {
        assert!(hevc_hw_encoder_available());
    }

    #[test]
    fn consecutive_sessions_produce_hevc_with_stable_frame_counts() {
        let Some(directory) = std::env::var_os("HONGGUO_E2E_DIR") else {
            return;
        };
        let source = std::path::Path::new(&directory).join("001.mp4");
        let run = |index| {
            let output = std::env::temp_dir()
                .join(format!("hongguo-hevc-{}-{index}.mp4", std::process::id()));
            transcode_hevc_for_tests(&hongguo_media_platform_api::PlatformRequest {
                input: &source,
                output: &output,
                scale_to: None,
                on_progress: None,
            })
            .expect("HEVC 源应适用平台转码")
            .expect("HEVC 编码应成功");
            let media = hongguo_media::demux::demux_file(&output).unwrap();
            let video = media.video_track().unwrap();
            assert!(hongguo_media::hevc::is_hevc(&video.info.codec));
            let count = video.info.samples.len();
            assert!(count > 0);
            std::fs::remove_file(output).unwrap();
            count
        };
        assert_eq!(run(1), run(2));
    }
}
