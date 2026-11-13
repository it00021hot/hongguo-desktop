use super::encode::{HEVC_MIME, HevcEncoder};
use hongguo_common::{AppError, AppResult};
use ndk::media::media_codec::MediaCodec;
use parking_lot::RwLock;

#[derive(Clone, Copy)]
struct EncoderSupport {
    available: bool,
    hardware: bool,
}

static HEVC_DEC: RwLock<Option<bool>> = RwLock::new(None);
static ENCODER: RwLock<Option<EncoderSupport>> = RwLock::new(None);

pub fn hevc_decoder_present() -> bool {
    if let Some(available) = *HEVC_DEC.read() {
        return available;
    }
    let available = MediaCodec::from_decoder_type(HEVC_MIME).is_some();
    *HEVC_DEC.write() = Some(available);
    available
}

pub fn encoder_available() -> bool {
    encoder_support().available
}

/// 已实编成功，且编码器名不是 Android/Google 软件组件。
pub fn hevc_hw_encoder_available() -> bool {
    encoder_support().hardware
}

pub fn clear_probe_cache() {
    *HEVC_DEC.write() = None;
    *ENCODER.write() = None;
}

fn encoder_support() -> EncoderSupport {
    if let Some(support) = *ENCODER.read() {
        return support;
    }
    let support = match try_encode() {
        Ok(hardware) => EncoderSupport {
            available: true,
            hardware,
        },
        Err(error) => {
            log::warn!("[Platform/android] HEVC 编码探测不可用: {error}");
            EncoderSupport {
                available: false,
                hardware: false,
            }
        }
    };
    *ENCODER.write() = Some(support);
    support
}

fn try_encode() -> AppResult<bool> {
    const WIDTH: usize = 256;
    const HEIGHT: usize = 256;
    let mut encoder = HevcEncoder::new(WIDTH, HEIGHT, 30.0, 800_000)?;
    encoder.push_nv12(&vec![128; WIDTH * HEIGHT * 3 / 2], 0, false)?;
    encoder.push_nv12(&[], 0, true)?;
    let output = encoder.pull()?;
    if output
        .first()
        .is_none_or(|(_, bytes, sync)| bytes.is_empty() || !sync)
    {
        return Err(AppError::Media("HEVC 编码探测没有同步样本".into()));
    }
    encoder.using_hardware()
}
