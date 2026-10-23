//! HEVC 解码：IMFSourceReader 文件流 → NV12 系统内存帧（转码链路用）。

use windows::Win32::Media::MediaFoundation::*;

use hongguo_common::{AppError, AppResult};

use super::mft::hr;
use super::sample::{sample_to_bytes, type_stride, video_type};

// ————————————————————————————————————————————————————————————
// 解码：HEVC MFT → NV12 系统内存
// ————————————————————————————————————————————————————————————

/// 一帧解码输出：连续 NV12 字节 + 自身 stride（别假设等于 width）。
pub(super) struct Nv12Frame {
    pub(super) data: Vec<u8>,
    pub(super) stride: usize,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) pts_100ns: i64,
    pub(super) dur_100ns: i64,
}

/// 解码器：**IMFSourceReader**（MF 的高层读入 API）。
///
/// 为什么不手工驱动解码 MFT：实机反复实证，微软 HEVC 扩展解码器手工
/// `ProcessInput`/`ProcessOutput` 会建会话成功但永远不出帧（序列头喂
/// hvcC 记录或 Annex-B 串、补 START_OF_STREAM、配 D3D11 管理器都救不回，
/// 后者还会段错误——该解码器疑似进程级单例 + DXVA 门控）；而
/// SourceReader 对同一文件稳定解出全部帧。它内部就是同一套解码 MFT +
/// 参数集/样本形态/DXVA 的正确装配，还自动吃到硬件解。h264 编码 MFT
/// 手工驱动则实证可用（NVIDIA/软编都通），两边的驱动契约不对称是 MF
/// 的现实，如实各取所长。
pub(super) struct Decoder {
    reader: IMFSourceReader,
    width: usize,
    height: usize,
    stride: usize,
}

impl Decoder {
    pub(super) fn new(input: &std::path::Path, width: usize, height: usize) -> AppResult<Self> {
        let wide: Vec<u16> = input
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let reader = hr(
                MFCreateSourceReaderFromURL(windows::core::PCWSTR(wide.as_ptr()), None),
                "建 SourceReader",
            )?;
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            hr(reader.SetStreamSelection(stream, true), "选视频流")?;
            let nv12 = video_type(MFVideoFormat_NV12)?;
            hr(
                reader.SetCurrentMediaType(stream, None, &nv12),
                "设 NV12 解码输出",
            )?;
            let cur = hr(reader.GetCurrentMediaType(stream), "读解码输出类型")?;
            let (w, h) = frame_size_of(&cur)?;
            // 解码面尺寸 ≠ 轨道声明尺寸是常态而非异常：编码器按对齐取整
            // （实测红果源声明 1922×1080，解码面 1928×1080）。原来严格相等
            // 校验直接拒了这类源，平台层全灭、静默落到十几分钟一集的软解。
            // 真正要拒的只有「解出来的画面比声明还小」（内容真缺了）或
            // 离谱地大（参数装配错了）；对齐差交给缩放/合并的统一分辨率去收。
            if w < width || h < height || w > width + 64 || h > height + 64 {
                return Err(AppError::Media(format!(
                    "解码分辨率 {w}x{h} 与轨道声明 {width}x{height} 相差过大"
                )));
            }
            let (w, h) = if (w, h) != (width, height) {
                log::info!("[Platform/mf] 解码面 {w}x{h}，轨道声明 {width}x{height}，按解码面走");
                (w, h)
            } else {
                (width, height)
            };
            let stride = type_stride(&cur, w);
            Ok(Decoder {
                reader,
                width: w,
                height: h,
                stride,
            })
        }
    }

    /// 下一帧（显示序）；`None` = 流结束。
    pub(super) fn next_frame(&mut self) -> AppResult<Option<Nv12Frame>> {
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        loop {
            let mut flags = 0u32;
            let mut sample = None;
            unsafe {
                hr(
                    self.reader.ReadSample(
                        stream,
                        0,
                        None,
                        Some(&mut flags),
                        None,
                        Some(&mut sample),
                    ),
                    "ReadSample",
                )?;
            }
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err(AppError::Media("解码流报告错误".into()));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            // 空节拍（STREAMTICK）与无样本回合：继续读
            let Some(s) = sample else { continue };
            if flags & MF_SOURCE_READERF_STREAMTICK.0 as u32 != 0 {
                continue;
            }
            let (data, t, d) = sample_to_bytes(&s)?;
            if data.len() < self.stride * self.height * 3 / 2 {
                return Err(AppError::Media(format!(
                    "解码帧字节不足（{} < 需要 {}）",
                    data.len(),
                    self.stride * self.height * 3 / 2
                )));
            }
            return Ok(Some(Nv12Frame {
                data,
                stride: self.stride,
                width: self.width,
                height: self.height,
                pts_100ns: t,
                dur_100ns: d,
            }));
        }
    }
}

/// 媒体类型的 `MF_MT_FRAME_SIZE` → (宽, 高)。
pub(super) fn frame_size_of(t: &IMFMediaType) -> AppResult<(usize, usize)> {
    let v = unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE) }
        .map_err(|e| AppError::Media(format!("Media Foundation 读帧尺寸失败（{e}）")))?;
    Ok(((v >> 32) as usize, (v & 0xffff_ffff) as usize))
}
