//! 探测与进程级缓存：平台硬编 / 软编 H.264 编码器可用性与 HEVC 解码 MFT 在位；
//! [`MF_VERSION`] 是会话线程初始化用的 mfapi 版本号。

use parking_lot::RwLock;

use windows::Win32::Media::MediaFoundation::*;

use hongguo_common::AppResult;

use super::decoder::Nv12Frame;
use super::encoder::{Encoder, EncoderKind};
use super::mft::{enum_activates, run_on_mf_thread, type_info};

/// mfapi.h：`MF_SDK_VERSION << 16 | MF_API_VERSION`（0x2 << 16 | 0x70）。
pub(super) const MF_VERSION: u32 = 0x20070;

static HW_ENCODER: RwLock<Option<bool>> = RwLock::new(None);
static SW_ENCODER: RwLock<Option<bool>> = RwLock::new(None);

/// 平台硬编 H.264 编码器是否可用（探测结果缓存）。
pub fn h264_hw_encoder_available() -> bool {
    let mut slot = HW_ENCODER.write();
    if slot.is_none() {
        *slot = Some(probe_h264_hw_encoder());
    }
    slot.unwrap_or(false)
}

/// in-box 软件 H.264 编码器（h264_mf）是否可用（探测结果缓存）。
pub fn h264_sw_encoder_available() -> bool {
    let mut slot = SW_ENCODER.write();
    if slot.is_none() {
        *slot = Some(probe_h264_sw_encoder());
    }
    slot.unwrap_or(false)
}

/// 丢弃探测缓存。
pub fn clear_probe_cache() {
    *HW_ENCODER.write() = None;
    *SW_ENCODER.write() = None;
}

fn probe_h264_hw_encoder() -> bool {
    match run_on_mf_thread(|| probe_encoder_kind(EncoderKind::Hardware)) {
        Ok(hw) => hw,
        Err(e) => {
            log::warn!("[Platform/mf] 平台硬编探测失败，按不可用处理: {e}");
            false
        }
    }
}

fn probe_h264_sw_encoder() -> bool {
    match run_on_mf_thread(|| probe_encoder_kind(EncoderKind::Software)) {
        Ok(ok) => ok,
        Err(e) => {
            log::warn!("[Platform/mf] 平台软编探测失败，按不可用处理: {e}");
            false
        }
    }
}

/// 探测：① HEVC 解码 MFT 在位（缺它平台转码没有意义）；② 指定档编码 MFT
/// 不止枚举得到，还要真实建会话实编一帧并**产出输出样本**。
fn probe_encoder_kind(kind: EncoderKind) -> AppResult<bool> {
    let decoders = enum_activates(
        MFT_CATEGORY_VIDEO_DECODER,
        MFT_ENUM_FLAG_SYNCMFT
            | MFT_ENUM_FLAG_ASYNCMFT
            | MFT_ENUM_FLAG_HARDWARE
            | MFT_ENUM_FLAG_SORTANDFILTER,
        Some(&type_info(MFMediaType_Video, MFVideoFormat_HEVC)),
        Some(&type_info(MFMediaType_Video, MFVideoFormat_NV12)),
    )?;
    if decoders.is_empty() {
        return Ok(false);
    }

    let mut encoder = Encoder::new(256, 256, 30, 1, 1_000_000, kind)?;
    encoder.encode(
        Nv12Frame {
            data: vec![0x80; 256 * 256 * 3 / 2],
            stride: 256,
            width: 256,
            height: 256,
            pts_100ns: 0,
            dur_100ns: 333_333,
        },
        0.0,
        1.0 / 30.0,
    )?;
    encoder.finish()?;
    Ok(!encoder.take_units()?.is_empty())
}
