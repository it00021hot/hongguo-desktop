//! 能力探测。
//!
//! 探测的是「当前这台机器转码会走哪条路」：
//! - 平台原生硬编可用（VideoToolbox / Media Foundation）→ 接近实时
//! - 有 ffmpeg 且选得到**真硬件**编码器 → 接近实时
//! - 有 ffmpeg 但只有软件编码器 → 中速
//! - 都没有 → 纯 Rust 软解，慢
//!
//! 探测会真的试编一帧，耗时一到两秒，所以结果全局缓存；点「重新检测」时丢弃缓存重探。

use parking_lot::RwLock;

use crate::media::Backend;

/// 探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeCapability {
    /// 装了 ffmpeg
    pub has_ffmpeg: bool,
    /// ffmpeg 侧有**真正走硬件**的 H.264 编码器（nvenc/qsv/amf/能开 `-hw_encoding` 的 mf）
    pub h264_hw_encoder: bool,
    /// 平台原生硬编可用（VideoToolbox / Media Foundation，随 GPU 驱动/系统提供，
    /// 不依赖用户装任何东西）
    pub platform_hw_encoder: bool,
}

static CAPABILITY: RwLock<Option<DecodeCapability>> = RwLock::new(None);

/// 取探测结果，没探过就探一次。
pub fn detect() -> DecodeCapability {
    let mut slot = CAPABILITY.write();
    if slot.is_none() {
        *slot = Some(probe());
    }
    slot.unwrap_or_default()
}

/// 丢弃缓存重新探测。
///
/// 装完 ffmpeg 之后**不重启应用是看不到变化的**——环境变量与探测缓存都不会自己更新。
/// 给出这条路后设置页就能提供「重新检测」，不必让用户去重启应用。
pub fn redetect() -> DecodeCapability {
    *CAPABILITY.write() = None;
    crate::media::ffmpeg::probe::reprobe();
    redetect_platform();
    let c = detect();
    let backend = crate::media::ffmpeg::backend_info();
    log::info!(
        "[Media] 重新探测：ffmpeg={} 编码器={} 硬件={} 平台={} → {}",
        c.has_ffmpeg,
        backend.encoder,
        c.h264_hw_encoder,
        c.platform_hw_encoder,
        backend.transcode_with
    );
    c
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn redetect_platform() {
    crate::media::platform::clear_probe_cache();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn redetect_platform() {}

fn probe() -> DecodeCapability {
    let has_ffmpeg = crate::media::ffmpeg::ffmpeg_path().is_some();
    // 「是不是硬件」由编码器自己报，**不能拿「不是 libx264」当硬件**：
    // h264_mf 默认走软件 MediaFoundation，把它算成硬件会让用户以为在用显卡。
    let hardware = crate::media::ffmpeg::h264_encoder().is_some_and(|e| e.hardware);
    DecodeCapability {
        has_ffmpeg,
        h264_hw_encoder: hardware,
        platform_hw_encoder: crate::media::platform::h264_hw_encoder_available(),
    }
}

/// 当前机器分流链会选中的最佳后端（不实际转码）。
///
/// 分流顺序与 [`crate::service::transcode_service::pipeline`] 一致：
/// 平台硬编 → ffmpeg → 纯 Rust。并行度、超订预期都从这里的结论出发。
pub fn selected_backend() -> Backend {
    if platform_hw() {
        return Backend::Platform;
    }
    match crate::media::ffmpeg::h264_encoder() {
        Some(e) if e.hardware => Backend::FfmpegHw,
        Some(_) => Backend::FfmpegSw,
        None => Backend::Rust,
    }
}

fn platform_hw() -> bool {
    !crate::media::platform::disabled_by_env() && crate::media::platform::h264_hw_encoder_available()
}

/// 当前机器的转码链路能否统一分辨率（缩放）。
///
/// ffmpeg 有 `scale` 滤镜，平台硬编有各自的像素搬移器（VT 的
/// `VTPixelTransferSession` / MF 的 VideoProcessor）；**纯 Rust 软解没有
/// 缩放能力**。没有缩放能力时，混合分辨率的剧集不能进兼容合并——
/// 否则转完几十分钟才死在拼接的宽高一致校验上。
pub fn scaling_available() -> bool {
    selected_backend() != Backend::Rust
}

/// 预热探测（启动时调用，避免首次转码时的额外延迟）。
pub fn warm_up() {
    let c = detect();
    let backend = crate::media::ffmpeg::backend_info();
    log::info!(
        "[Media] ffmpeg={} 编码器={} 硬件={} 平台={} → {}",
        c.has_ffmpeg,
        backend.encoder,
        c.h264_hw_encoder,
        c.platform_hw_encoder,
        backend.transcode_with
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_is_cached_and_stable() {
        let a = detect();
        let b = detect();
        assert_eq!(a, b, "探测结果应被缓存，不重复试编");
    }

    #[test]
    fn capability_is_serializable() {
        // 反序列化回来必须字段无损，前端直接消费这个结构
        let cap = detect();
        let json = serde_json::to_string(&cap).unwrap();
        let back: DecodeCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back, "序列化往返应无损");
    }

    #[test]
    fn redetect_keeps_the_answer_stable() {
        // 重新探测不该改变结论——同一台机器同一个答案。
        // 这条防的是「重探把已经选好的编码器换掉」。
        let before = detect();
        let after = redetect();
        assert_eq!(before, after, "同一台机器重复探测应得到相同结论");
    }

    #[test]
    fn hardware_flag_is_not_derived_from_the_encoder_name() {
        // 回归钉住：曾经用 `encoder != "libx264"` 判硬件，
        // 于是 h264_mf（软件 MediaFoundation）被报成「硬件加速」。
        if let Some(enc) = crate::media::ffmpeg::h264_encoder() {
            if enc.name == "h264_mf" && !enc.hardware {
                // 本机 h264_mf 只能软件编码：能力字段必须如实报 false
                assert!(
                    !detect().h264_hw_encoder
                        || crate::media::ffmpeg::h264_encoder().is_some_and(|e| e.hardware),
                    "软件编码的 h264_mf 不能报成硬件"
                );
            }
        }
    }
}
