//! Apple HEVC 解码、NV12 缩放及 HEVC 编码，保留源显示时间戳。

mod decode;
mod encode;
mod ffi;
mod hardware;
mod pipeline;
mod pixel;
mod probe;
mod yuv;

pub use pipeline::transcode_hevc;
pub use probe::{clear_probe_cache, encoder_available, hevc_hw_encoder_available};
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
        let _ = hevc_hw_encoder_available();
    }
}
