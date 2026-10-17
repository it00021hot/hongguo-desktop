//! 视频帧与解码器契约：平台实现与业务调用方共同依赖的数据形态。
//!
//! 帧类型 [`VideoFrame`] 对两端的差异（像素载体）做了单一封装：macOS 带
//! `CVPixelBuffer`（渲染零拷贝上屏），Windows 带 [`Nv12Plane`]（Arc 共享，
//! 进帧槽/事件零复制）。解码器生命周期由 [`VideoDecoder`] 表达（构造由各
//! 实现以固有方法给出，decode/finish 为契约行为）；具体类型分居
//! `hongguo-media-platform-macos` / `hongguo-media-platform-windows`，
//! 门面 `hongguo-media-platform` 按编译目标 re-export
//! （`hongguo_media_platform::platform::video::VideoFrameDecoder`）。
//!
//! 统一调用契约不代表统一底层内存布局：macOS 的 CVPixelBuffer 路径与
//! Windows 的 NV12 系统内存帧路径各自保留优势，下游按具体类型静态分发。

#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;

use hongguo_common::AppResult;

/// 解码出的一帧：PTS（秒，显示时间）+ 像素载体。
#[derive(Clone)]
pub struct VideoFrame {
    pub pts_secs: f64,
    pub dims: (usize, usize),
    pub buffer: FrameBuffer,
}

// 跨线程搬运的约束由本声明给出：macOS 的 CVPixelBuffer 是引用计数线程安全
// 的 CF 对象（vt 侧的 DecodedFrame 同款约定），Windows 的 Arc<Vec<u8>> 天然
// 满足。引擎任务经 tokio::spawn 传帧、宿主经事件通道收帧都依赖它。
unsafe impl Send for VideoFrame {}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("pts_secs", &self.pts_secs)
            .field("dims", &self.dims)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub enum FrameBuffer {
    /// macOS：NV12 CVPixelBuffer（gpui surface 零拷贝；引用计数克隆）。
    #[cfg(target_os = "macos")]
    CVPixelBuffer(CVPixelBuffer),
    /// Windows：NV12 系统内存（`data` 布局 = Y 平面 `stride×height` +
    /// UV 交错平面 `stride×height/2`）。
    #[cfg(target_os = "windows")]
    Nv12(std::sync::Arc<Nv12Plane>),
}

/// Windows 的 NV12 系统内存帧。
#[cfg(target_os = "windows")]
pub struct Nv12Plane {
    pub data: std::sync::Arc<Vec<u8>>,
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

/// 视频编码：解码侧要按它选 MFT / VT 会话与序列头形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// H.265（HEVC）：序列头是 hvcC。
    Hevc,
    /// H.264（AVC）：序列头是 avcC。
    H264,
}

/// 建解码器的参数。
pub struct DecoderInit<'a> {
    pub codec: VideoCodec,
    /// 参数集裸 NAL（VPS/SPS/PPS）：macOS VT 的指定形态。
    pub parameter_sets: &'a [Vec<u8>],
    /// 序列头原始载荷（HEVC 的 hvcC / H.264 的 avcC）：Windows MF 的
    /// `MF_MT_MPEG_SEQUENCE_HEADER` 形态。两端各自只认自己那份，缺了对应端
    /// 会走带内参数集等兜底。
    pub sequence_header: Option<&'a [u8]>,
    /// 样本的 NAL 长度前缀字节数（带内参数集前插时用，1–4）。
    pub length_size: usize,
    /// 轨道声明分辨率（MF 输入类型要帧尺寸；VT 自行从参数集读）。
    pub width: usize,
    pub height: usize,
    /// 请求 FullRange NV12。**macOS 已不消费**：VT 恒按硬件原生 '420v'
    /// （视频区间）输出并由渲染端直接采样（请求 '420f' 会让 VT 逐帧插入
    /// 色域换算，实测吞吐差一个数量级）；Windows MF 输出恒 limited，由渲染
    /// 转换端按 BT.709 limited 处理。字段保留在契约里供其它平台实现使用。
    pub full_range: bool,
}

/// 帧流解码器契约（HEVC / H.264）：有状态会话的生命周期与行为。
///
/// 构造（open）由各平台实现以固有方法给出——参数形态两端不同（VT 吃
/// 裸参数集，MF 吃序列头 + 帧尺寸），强行统一构造签名只会把平台细节
/// 泄进契约。本 trait 约束的是会话行为：逐样本解码与冲尾。
pub trait VideoDecoder: Send {
    /// 解一个样本（MP4 长度前缀 NAL 原样字节），返回本次产生的帧（解码序，
    /// 各自带 PTS；无输出返回空表）。
    fn decode(
        &mut self,
        sample: &[u8],
        pts_secs: f64,
        duration_secs: f64,
    ) -> AppResult<Vec<VideoFrame>>;

    /// 冲尾帧并返回遗留输出（会话随即不再可用）。
    fn finish(&mut self) -> AppResult<Vec<VideoFrame>>;
}

/// 码率标定：VT 没有 CRF、MF 的 MFT 同理，按「0.12 bit/像素/帧」给出与
/// libx264 `-crf 23` 同档的码率（1080p25 ≈ 6.2 Mbps）。mf/vt 两侧原本各
/// 写一份逐字节相同——2026-10-07 在 Windows 侧用 NVENC MFT + libvmaf 双源
/// 复核后从 0.1 提到 0.12（中低复杂度近真人剧 VMAF 98.0 略优于 crf23 的
/// 97.1；高运动内容 CBR 固定码率天然吃亏，0.1 系数时 86.5 vs 92.4，提到
/// 0.12 收窄到 ~3 分）。再偏差大时只调这里的系数。
pub fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * 0.12;
    bps.clamp(800_000.0, 12_000_000.0) as i32
}
