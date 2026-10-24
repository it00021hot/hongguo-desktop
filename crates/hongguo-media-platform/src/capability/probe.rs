//! 能力探测。
//!
//! 探测的是「当前这台机器转码会走哪条路」：
//! - 平台原生编码层可用（macOS：VT 会话可建即真，含 Apple 软编；Windows：
//!   有硬件 MFT）→ 接近实时到中速（视硬编与否）
//! - 有 ffmpeg 且选得到**真硬件**编码器 → 接近实时
//! - 有 ffmpeg 但只有软件编码器 → 中速
//! - 都没有 → **没有转码后端**，转码做不了
//!
//! 成本分层：平台层是纯原生 API（MFTEnumEx / VT 会话），毫秒级；`has_ffmpeg`
//! 只要一次 `ffmpeg -version`；ffmpeg 侧「真硬件」要 `-encoders` + 逐个试编
//! 一帧（一到两秒的子进程链）。分流第一级是平台层（[`selected_backend`]），
//! 它可用时 ffmpeg 硬编结论对设置页标签和实际链路都无发言权——probe 因此
//! 跳过试编层（见 [`probe`]），平台层不可用（ffmpeg 升级为兜底决定者）或
//! 用户点「重新检测」前的兜底路径才付这笔钱。结果全局缓存。

use parking_lot::RwLock;

use hongguo_media::Backend;

/// 探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeCapability {
    /// 装了 ffmpeg
    pub has_ffmpeg: bool,
    /// ffmpeg 侧有**真正走硬件**的 H.264 编码器（nvenc/qsv/amf/能开 `-hw_encoding` 的 mf）
    pub h264_hw_encoder: bool,
    /// 平台原生编码层可用（macOS：VT 会话可建即真，含 Apple 软编会话；
    /// Windows：有硬件 MFT），不依赖用户装任何东西
    pub platform_encoder: bool,
    /// 平台原生**硬编**可用——只用于徽标「硬件加速」档与并行度，
    /// 不是 macOS 生产闸门（软编会话同样进生产）
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
    crate::ffmpeg::probe::reprobe();
    redetect_platform();
    let c = detect();
    let backend = crate::ffmpeg::backend_info();
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
    crate::platform::clear_probe_cache();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn redetect_platform() {}

fn probe() -> DecodeCapability {
    // 严格按分流优先级探测：平台层（纯原生 API，毫秒级）先探；它可用时
    // ffmpeg 整层不探——链路不会用它（selected_backend 直接走平台），装
    // ffmpeg 没有任何收益，设置页安装指引也按同一条件隐藏（「装上后转码
    // 自动改走它」在那档机器上是假话）。平台层不可用，ffmpeg 升级为链路
    // 兜底与指引的决定者，才付探测的钱（-version 约 50ms；真硬件还要
    // -encoders + 试编）。转码若在运行期回落 ffmpeg，h264_encoder() 的
    // 惰性缓存会在那一刻自己探，与本缓存无关。
    let (has_ffmpeg, h264_hw) = if platform_available() {
        (false, false)
    } else {
        (
            crate::ffmpeg::ffmpeg_path().is_some(),
            crate::ffmpeg::h264_encoder().is_some_and(|e| e.hardware),
        )
    };
    DecodeCapability {
        has_ffmpeg,
        h264_hw_encoder: h264_hw,
        platform_encoder: crate::platform::encoder_available(),
        platform_hw_encoder: crate::platform::h264_hw_encoder_available(),
    }
}

/// 当前机器分流链会选中的后端（不实际转码）。
///
/// 分流顺序与壳内 `service::transcode_service::pipeline` 一致：
/// 平台编码层 → ffmpeg。**`None` = 一条路都没有**（平台编码器不可用且没装
/// ffmpeg）——此时转码根本做不了，调用方必须自己处理这个状态。
pub fn selected_backend() -> Option<Backend> {
    if platform_available() {
        return Some(Backend::Platform);
    }
    match crate::ffmpeg::h264_encoder() {
        Some(e) if e.hardware => Some(Backend::FfmpegHw),
        Some(_) => Some(Backend::FfmpegSw),
        None => None,
    }
}

fn platform_available() -> bool {
    !crate::platform::disabled_by_env() && crate::platform::encoder_available()
}

/// 预热探测（启动时调用，避免首次转码时的额外延迟）。
pub fn warm_up() {
    let c = detect();
    if c.platform_encoder {
        // 平台层可用＝分流已定（selected_backend 直接走平台），ffmpeg 整层
        // 都没探（probe 有意跳过）。日志不能再调 backend_info()——它内部
        // 就是 ffmpeg_path()+h264_encoder()，会把省掉的探测全触发回来。
        log::info!(
            "[Media] 平台编码层可用（硬件={}）→ 分流已定，ffmpeg 兜底层不探测",
            c.platform_hw_encoder
        );
        return;
    }
    let backend = crate::ffmpeg::backend_info();
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
        if let Some(enc) = crate::ffmpeg::h264_encoder()
            && enc.name == "h264_mf"
            && !enc.hardware
        {
            // 本机 h264_mf 只能软件编码：能力字段必须如实报 false
            assert!(
                !detect().h264_hw_encoder
                    || crate::ffmpeg::h264_encoder().is_some_and(|e| e.hardware),
                "软件编码的 h264_mf 不能报成硬件"
            );
        }
    }
}
