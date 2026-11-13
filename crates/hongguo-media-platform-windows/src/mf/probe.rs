//! 探测与进程级缓存：平台硬编 / 软编 HEVC 编码器可用性与 HEVC 解码 MFT 在位；
//! [`MF_VERSION`] 是会话线程初始化用的 mfapi 版本号。

use parking_lot::RwLock;

use hongguo_common::AppResult;

use super::decoder::Nv12Frame;
use super::encoder::{Encoder, EncoderKind};
use super::mft::run_on_mf_thread;

/// mfapi.h：`MF_SDK_VERSION << 16 | MF_API_VERSION`（0x2 << 16 | 0x70）。
pub(super) const MF_VERSION: u32 = 0x20070;

static HW_ENCODER: RwLock<Option<bool>> = RwLock::new(None);
static SW_ENCODER: RwLock<Option<bool>> = RwLock::new(None);

/// 平台硬编 HEVC 编码器是否可用（真实编码结果缓存）。
pub fn hevc_hw_encoder_available() -> bool {
    let mut slot = HW_ENCODER.write();
    if slot.is_none() {
        *slot = Some(probe_hevc_hw_encoder());
    }
    slot.unwrap_or(false)
}

/// 软件 HEVC 编码器是否可用（真实编码结果缓存）。
pub fn hevc_sw_encoder_available() -> bool {
    let mut slot = SW_ENCODER.write();
    if slot.is_none() {
        *slot = Some(probe_hevc_sw_encoder());
    }
    slot.unwrap_or(false)
}

/// 丢弃探测缓存。
pub fn clear_probe_cache() {
    *HW_ENCODER.write() = None;
    *SW_ENCODER.write() = None;
}

fn probe_hevc_hw_encoder() -> bool {
    match run_on_mf_thread(|| probe_encoder_kind(EncoderKind::Hardware)) {
        Ok(hw) => hw,
        Err(e) => {
            log::warn!("[Platform/mf] 平台硬编探测失败，按不可用处理: {e}");
            false
        }
    }
}

fn probe_hevc_sw_encoder() -> bool {
    match run_on_mf_thread(|| probe_encoder_kind(EncoderKind::Software)) {
        Ok(ok) => ok,
        Err(e) => {
            log::warn!("[Platform/mf] 平台软编探测失败，按不可用处理: {e}");
            false
        }
    }
}

fn probe_encoder_kind(kind: EncoderKind) -> AppResult<bool> {
    let mut encoder = Encoder::new(256, 256, 30, 1, 1_000_000, kind)?;
    for index in 0..2 {
        encoder.encode(
            Nv12Frame {
                data: vec![0x80; 256 * 256 * 3 / 2],
                stride: 256,
                width: 256,
                height: 256,
                pts_100ns: i64::from(index) * 333_333,
                dur_100ns: 333_333,
            },
            f64::from(index) / 30.0,
            1.0 / 30.0,
        )?;
    }
    encoder.finish()?;
    Ok(encoder.take_units()?.len() == 2)
}
