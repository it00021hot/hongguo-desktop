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
//! `media::hevc::to_annexb`。时间戳同样读**输出样本自带的 PTS**（100ns），
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

// windows-rs 的事件类型常量（METransformNeedInput 等）沿用 MF 的混合大小写
// 命名，出现在 match 模式里会触发 non_upper_case_globals。
#![allow(non_upper_case_globals)]

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0_0, VARIANT_0_0_0, VT_UI4};
use windows::core::{GUID, Interface};

use crate::domain::mp4::sample_table::TrackInfo;
use crate::error::{AppError, AppResult};

use super::PlatformRequest;

/// mfapi.h：`MF_SDK_VERSION << 16 | MF_API_VERSION`（0x2 << 16 | 0x70）。
const MF_VERSION: u32 = 0x20070;

static HW_ENCODER: RwLock<Option<bool>> = RwLock::new(None);

/// 平台硬编 H.264 编码器是否可用（探测结果缓存）。
pub fn h264_hw_encoder_available() -> bool {
    let mut slot = HW_ENCODER.write();
    if slot.is_none() {
        *slot = Some(probe_h264_hw_encoder());
    }
    slot.unwrap_or(false)
}

/// 丢弃探测缓存。
pub fn clear_probe_cache() {
    *HW_ENCODER.write() = None;
}

/// 平台硬编可用时执行转码；不可用返回 `None`（HEVC 以外的源也返回 `None`，
/// 留给 ffmpeg/软解——H.264 源在 ffmpeg 侧是直转，没必要绕 GPU）。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    if !h264_hw_encoder_available() {
        return None;
    }
    let demuxed = crate::media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !crate::media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run_on_mf_thread(|| {
        run(req, &demuxed, &video.info, EncoderKind::Hardware)
    }))
}

/// 仅测试构建：以「回落软件编码器」的策略跑完整链路，让没有硬编的机器
/// 也能验证管线本身（解码/Annex-B/封装/时间戳）。硬件与否由探测测试另行验证。
#[cfg(test)]
pub fn transcode_h264_for_tests(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let demuxed = crate::media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !crate::media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run_on_mf_thread(|| {
        run(req, &demuxed, &video.info, EncoderKind::Software)
    }))
}

fn probe_h264_hw_encoder() -> bool {
    match run_on_mf_thread(probe_reports_hardware) {
        Ok(hw) => hw,
        Err(e) => {
            log::warn!("[Platform/mf] 平台硬编探测失败，按不可用处理: {e}");
            false
        }
    }
}

/// 探测：① HEVC 解码 MFT 在位（缺它平台层没有意义）；② 硬件编码 MFT 不止
/// 枚举得到，还要真实建会话实编一帧并**产出输出样本**。
fn probe_reports_hardware() -> AppResult<bool> {
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

    let mut encoder = Encoder::new(256, 256, 30, 1, 1_000_000, EncoderKind::Hardware)?;
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

// ————————————————————————————————————————————————————————————
// 会话线程：COM/MTA + MFStartup 的生命周期
// ————————————————————————————————————————————————————————————

/// COM(MTA) + MF 的进程级初始化：**一次初始化，永不拆卸**。
///
/// 按会话配对 `CoUninitialize`/`MFShutdown` 会触发 COM 拆卸与 MF 组件后台
/// 线程（实测微软 HEVC 扩展解码器带常驻工作线程）的退出竞态：全量测试 +
/// MF 会话并行的进程里，~1/3 在**全部用例通过之后**的退出阶段于
/// ntdll 固定偏移处访问违例（WER 事件日志实证）。常驻进程（本应用就是）
/// 一次初始化、交给进程退出统一回收是标准做法，没有泄漏语义。
static SESSION_INIT: std::sync::Once = std::sync::Once::new();

/// 在专用线程上跑 MF 会话。scoped 线程允许闭包借用非 'static 数据
/// （请求/解复用结果），COM 状态不落在 tokio 工作线程上。
fn run_on_mf_thread<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    std::thread::scope(|s| {
        s.spawn(move || {
            SESSION_INIT.call_once(|| {
                // RPC_E_CHANGED_MODE：进程已按别的模式初始化过 COM——
                // 沿用现状即可，MTA 是否由本调用建立不影响会话正确性。
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                if let Err(e) = unsafe { MFStartup(MF_VERSION, 0) } {
                    log::error!("[Platform/mf] MFStartup 失败: {e}");
                }
            });
            f()
        })
        .join()
        .map_err(|_| AppError::Media("Media Foundation 线程异常退出".into()))?
    })
}

/// MF 调用错误的统一包装（对齐 vt 侧的 `os()`）。
fn hr<T>(r: windows::core::Result<T>, what: &str) -> AppResult<T> {
    r.map_err(|e| AppError::Media(format!("Media Foundation {what}失败（{e}）")))
}

fn is_code(e: &windows::core::Error, code: windows::core::HRESULT) -> bool {
    e.code() == code
}

/// 经 ICodecAPI 设一个 u32 码控参数（尽力而为，被拒只记日志）。
fn set_codecapi_u32(xform: &IMFTransform, api: &GUID, value: u32, label: &str) {
    let Ok(icapi) = xform.cast::<ICodecAPI>() else {
        return;
    };
    let mut v = VARIANT::default();
    v.Anonymous.Anonymous = std::mem::ManuallyDrop::new(VARIANT_0_0 {
        vt: VT_UI4,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: VARIANT_0_0_0 { ulVal: value },
    });
    unsafe {
        if let Err(e) = icapi.SetValue(api, &v) {
            log::debug!("[Platform/mf] 编码器拒收 {label}: {e}");
        }
    }
}

// ————————————————————————————————————————————————————————————
// MFT 枚举与激活
// ————————————————————————————————————————————————————————————

fn type_info(major: GUID, subtype: GUID) -> MFT_REGISTER_TYPE_INFO {
    MFT_REGISTER_TYPE_INFO {
        guidMajorType: major,
        guidSubtype: subtype,
    }
}

/// 枚举 MFT（按 `SORTANDFILTER` 的排序）。注意：**没有**
/// `MFT_ENUM_FLAG_SOFTWARE` 这个标志——软件 MFT 用 `SYNCMFT`（不带
/// HARDWARE）枚举，这是对 mfapi.h 核实过的事实，别凭直觉加。
fn enum_activates(
    category: GUID,
    flags: MFT_ENUM_FLAG,
    input: Option<&MFT_REGISTER_TYPE_INFO>,
    output: Option<&MFT_REGISTER_TYPE_INFO>,
) -> AppResult<Vec<IMFActivate>> {
    unsafe {
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut n = 0u32;
        hr(
            MFTEnumEx(
                category,
                flags,
                input.map(|p| p as *const _),
                output.map(|p| p as *const _),
                &mut list,
                &mut n,
            ),
            "MFTEnumEx",
        )?;
        let mut out = Vec::with_capacity(n as usize);
        if !list.is_null() {
            for i in 0..n as usize {
                if let Some(a) = (*list.add(i)).take() {
                    out.push(a);
                }
            }
            CoTaskMemFree(Some(list as *const _));
        }
        Ok(out)
    }
}

/// 激活一个 MFT。异步契约的 MFT 必须先在属性表上解锁
/// （`MF_TRANSFORM_ASYNC_UNLOCK=1`），否则一切调用都回
/// `MF_E_TRANSFORM_ASYNC_LOCKED`。
fn activate_transform(activate: &IMFActivate) -> AppResult<(IMFTransform, bool)> {
    unsafe {
        let xform: IMFTransform = hr(activate.ActivateObject(), "激活 MFT")?;
        let attrs = hr(xform.GetAttributes(), "读 MFT 属性表")?;
        let is_async = attrs.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0;
        if is_async {
            hr(
                attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1),
                "解锁异步 MFT",
            )?;
        }
        Ok((xform, is_async))
    }
}

// ————————————————————————————————————————————————————————————
// 媒体类型与样本小工具
// ————————————————————————————————————————————————————————————

const HNS: f64 = 10_000_000.0;

fn secs_to_hns(s: f64) -> i64 {
    (s * HNS).round() as i64
}

/// 一个空的视频媒体类型（major=Video + 指定 subtype）。
fn video_type(subtype: GUID) -> AppResult<IMFMediaType> {
    let t = hr(unsafe { MFCreateMediaType() }, "建媒体类型")?;
    unsafe {
        hr(
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video),
            "设 major type",
        )?;
        hr(t.SetGUID(&MF_MT_SUBTYPE, &subtype), "设 subtype")?;
    }
    Ok(t)
}

/// 帧尺寸/帧率/逐行扫描三件套（UINT64 高 32 位与低 32 位打包）。
fn set_size_rate(t: &IMFMediaType, w: u32, h: u32, fps_num: u32, fps_den: u32) -> AppResult<()> {
    unsafe {
        hr(
            t.SetUINT64(&MF_MT_FRAME_SIZE, (u64::from(w) << 32) | u64::from(h)),
            "设帧尺寸",
        )?;
        hr(
            t.SetUINT64(
                &MF_MT_FRAME_RATE,
                (u64::from(fps_num) << 32) | u64::from(fps_den),
            ),
            "设帧率",
        )?;
        hr(
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
            "设扫描方式",
        )?;
    }
    Ok(())
}

/// 媒体类型上的 NV12 stride（字节行距）。缺失时按宽度兜底。
fn type_stride(t: &IMFMediaType, width: usize) -> usize {
    unsafe { t.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
        .map(|v| v as usize)
        .unwrap_or(width)
}

/// NV12 平面帧 → 样本（按目标 stride 逐行拷贝，不假设 stride=width）。
fn nv12_sample(
    data: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    dst_stride: usize,
    pts_100ns: i64,
    dur_100ns: i64,
) -> AppResult<IMFSample> {
    let h_uv = h.div_ceil(2);
    let total = dst_stride * (h + h_uv);
    unsafe {
        let sample = hr(MFCreateSample(), "建视频样本")?;
        let buf = hr(MFCreateMemoryBuffer(total as u32), "建视频缓冲")?;
        let mut ptr = std::ptr::null_mut();
        hr(buf.Lock(&mut ptr, None, None), "锁视频缓冲")?;
        let dst = std::slice::from_raw_parts_mut(ptr, total);
        for row in 0..h {
            let end = row * src_stride + w;
            if end > data.len() {
                return Err(AppError::Media("NV12 源帧越界（Y 平面）".into()));
            }
            let d = row * dst_stride + w;
            if d > total {
                return Err(AppError::Media("NV12 目标缓冲越界（Y 平面）".into()));
            }
            dst[row * dst_stride..d].copy_from_slice(&data[row * src_stride..end]);
        }
        let uv_src = src_stride * h;
        let uv_dst = dst_stride * h;
        for row in 0..h_uv {
            let s = uv_src + row * src_stride;
            let e = s + w;
            let d = uv_dst + row * dst_stride + w;
            if e > data.len() || d > total {
                return Err(AppError::Media("NV12 拷贝越界（UV 平面）".into()));
            }
            dst[uv_dst + row * dst_stride..d].copy_from_slice(&data[s..e]);
        }
        let _ = buf.Unlock();
        hr(buf.SetCurrentLength(total as u32), "设视频长度")?;
        hr(sample.AddBuffer(&buf), "挂视频缓冲")?;
        hr(sample.SetSampleTime(pts_100ns), "设视频时间")?;
        hr(sample.SetSampleDuration(dur_100ns), "设视频时长")?;
        Ok(sample)
    }
}

/// 样本 → 连续字节（连续化 + Lock 拷贝）。返回 `(字节, 时间戳, 时长)`，
/// 时间戳缺失时为 -1（交给上层递增兜底）。
fn sample_to_bytes(sample: &IMFSample) -> AppResult<(Vec<u8>, i64, i64)> {
    unsafe {
        let buf: IMFMediaBuffer = hr(sample.ConvertToContiguousBuffer(), "连续化样本")?;
        let mut ptr = std::ptr::null_mut();
        let mut cur = 0u32;
        hr(buf.Lock(&mut ptr, None, Some(&mut cur)), "锁样本")?;
        let bytes = std::slice::from_raw_parts(ptr, cur as usize).to_vec();
        let _ = buf.Unlock();
        let t = sample.GetSampleTime().unwrap_or(-1);
        let d = sample.GetSampleDuration().unwrap_or(-1);
        Ok((bytes, t, d))
    }
}

/// 输出样本是否可独立解码（关键帧）。属性缺失按非关键帧处理——
/// 宁缺毋滥：标错关键帧会毁掉产物的 seek。
fn sample_is_clean(sample: &IMFSample) -> bool {
    unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0
}

// ————————————————————————————————————————————————————————————
// 同步 MFT 驱动（in-box 软件 MFT 的契约）
// ————————————————————————————————————————————————————————————

enum Pulled {
    NeedMoreInput,
    Sample(IMFSample),
    StreamChange,
}

/// 同步 MFT：输出缓冲要么 MFT 自己给（`PROVIDES_SAMPLES`），要么按
/// `GetOutputStreamInfo` 的 `cbSize` 分配。类型协商后要 `refresh_out_info`。
struct SyncMft {
    xform: IMFTransform,
    provides_samples: bool,
    out_size: u32,
}

impl SyncMft {
    fn refresh_out_info(&mut self) -> AppResult<()> {
        unsafe {
            let info = hr(self.xform.GetOutputStreamInfo(0), "读输出流信息")?;
            self.provides_samples =
                (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            self.out_size = info.cbSize;
        }
        Ok(())
    }

    fn alloc_out_sample(&self) -> Option<IMFSample> {
        if self.provides_samples || self.out_size == 0 {
            return None;
        }
        (|| -> AppResult<Option<IMFSample>> {
            unsafe {
                let sample = hr(MFCreateSample(), "建输出样本")?;
                let buf = hr(MFCreateMemoryBuffer(self.out_size), "建输出缓冲")?;
                hr(sample.AddBuffer(&buf), "挂输出缓冲")?;
                Ok(Some(sample))
            }
        })()
        .map_err(|e| {
            log::warn!("[Platform/mf] 分配输出缓冲失败: {e}");
            e
        })
        .ok()
        .flatten()
    }

    fn pull_once(&mut self) -> AppResult<Pulled> {
        unsafe {
            let buf = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(self.alloc_out_sample()),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };
            let mut arr = [buf];
            let mut status = 0u32;
            let r = self.xform.ProcessOutput(0, &mut arr, &mut status);
            let buf = &mut arr[0];
            match r {
                Ok(()) => {
                    let s = std::mem::ManuallyDrop::take(&mut buf.pSample);
                    Ok(match s {
                        Some(s) => Pulled::Sample(s),
                        None => Pulled::NeedMoreInput,
                    })
                }
                Err(e) if is_code(&e, MF_E_TRANSFORM_NEED_MORE_INPUT) => Ok(Pulled::NeedMoreInput),
                Err(e) if is_code(&e, MF_E_TRANSFORM_STREAM_CHANGE) => Ok(Pulled::StreamChange),
                Err(e) => Err(AppError::Media(format!(
                    "Media Foundation ProcessOutput 失败（{e}）"
                ))),
            }
        }
    }
}

// ————————————————————————————————————————————————————————————
// 异步 MFT 驱动（硬件 MFT 的契约：事件循环）
// ————————————————————————————————————————————————————————————

/// 异步 MFT：只有收到 `METransformNeedInput` 才能喂；`METransformHaveOutput`
/// 后调 `ProcessOutput` 取样本。NeedInput 到来而队列空时记 `need_input`，
/// 下一个入队样本直接补发——错过一个事件就永远喂不进了。
struct AsyncMft {
    xform: IMFTransform,
    event_gen: IMFMediaEventGenerator,
    input_q: VecDeque<IMFSample>,
    need_input: bool,
    drained: bool,
    provides_samples: bool,
    out_size: u32,
}

impl AsyncMft {
    /// 非阻塞收一轮事件。
    fn poll(&mut self, ready: &mut Vec<IMFSample>) -> AppResult<()> {
        unsafe {
            loop {
                let ev = match self.event_gen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    Ok(ev) => ev,
                    Err(e) if is_code(&e, MF_E_NO_EVENTS_AVAILABLE) => break,
                    Err(e) => {
                        return Err(AppError::Media(format!(
                            "Media Foundation 事件循环失败（{e}）"
                        )));
                    }
                };
                let ty = MF_EVENT_TYPE(ev.GetType().map_err(|e| {
                    AppError::Media(format!("Media Foundation 读事件类型失败（{e}）"))
                })? as i32);
                match ty {
                    METransformNeedInput => match self.input_q.pop_front() {
                        Some(s) => hr(self.xform.ProcessInput(0, &s, 0), "ProcessInput")?,
                        None => {
                            self.need_input = true;
                            break;
                        }
                    },
                    METransformHaveOutput => ready.push(self.pull_one()?),
                    METransformMarker => {}
                    METransformDrainComplete => {
                        self.drained = true;
                        break;
                    }
                    _ => {}
                }
            }
            if self.need_input
                && let Some(s) = self.input_q.pop_front()
            {
                hr(self.xform.ProcessInput(0, &s, 0), "ProcessInput")?;
                self.need_input = false;
            }
        }
        Ok(())
    }

    fn pull_one(&mut self) -> AppResult<IMFSample> {
        unsafe {
            let sample = if self.provides_samples {
                None
            } else {
                let s = hr(MFCreateSample(), "建输出样本")?;
                let buf = hr(MFCreateMemoryBuffer(self.out_size), "建输出缓冲")?;
                hr(s.AddBuffer(&buf), "挂输出缓冲")?;
                Some(s)
            };
            let mut arr = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            hr(
                self.xform.ProcessOutput(0, &mut arr, &mut status),
                "ProcessOutput",
            )?;
            std::mem::ManuallyDrop::take(&mut arr[0].pSample)
                .ok_or_else(|| AppError::Media("Media Foundation 输出样本缺失".into()))
        }
    }

    /// 排空异步 MFT。顺序契约：
    /// ① **把队列里没喂完的样本全部喂完**才能发 `END_OF_STREAM`——NeedInput
    ///    事件的到达可能滞后（实测首个会话之后事件明显变慢），提早宣告流
    ///    结束会让 MFT 只编码已收到的帧、剩下的整段丢掉；
    /// ② `END_OF_STREAM` 之后**还要**发 `COMMAND_DRAIN`，否则有的 MFT
    ///    （实测 NVIDIA 编码 MFT）永远不发 `METransformDrainComplete`。
    /// 全程带超时兜底（MFT 卡死时不拖死整个转码）。
    fn drain(&mut self, ready: &mut Vec<IMFSample>) -> AppResult<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !self.input_q.is_empty() && !self.drained {
            if Instant::now() > deadline {
                return Err(AppError::Media("Media Foundation 异步 MFT 喂入超时".into()));
            }
            self.poll(ready)?;
            if !self.input_q.is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        unsafe {
            hr(
                self.xform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0),
                "通知流结束",
            )?;
            hr(
                self.xform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0),
                "命令排空",
            )?;
        }
        while !self.drained {
            if Instant::now() > deadline {
                return Err(AppError::Media("Media Foundation 异步 MFT 收尾超时".into()));
            }
            self.poll(ready)?;
            if !self.drained {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.poll(ready)
    }
}

// ————————————————————————————————————————————————————————————
// 统一驱动：解码器/编码器不关心同步还是异步
// ————————————————————————————————————————————————————————————

/// MFT 的统一驱动：输入侧 `push`，输出侧攒在 `ready` 里由 `take` 收割。
/// 同步 MFT 解码中可能报输出类型变化（分辨率变化流），置 sticky 标志
/// `stream_changed`，由 Decoder 重谈输出类型后继续。
struct Driver {
    inner: DriverInner,
    ready: Vec<IMFSample>,
    stream_changed: bool,
}

enum DriverInner {
    Sync(SyncMft),
    Async(AsyncMft),
}

impl Driver {
    fn new(activate: &IMFActivate) -> AppResult<(Self, IMFTransform)> {
        let (xform, is_async) = activate_transform(activate)?;
        let inner = if is_async {
            let event_gen: IMFMediaEventGenerator = hr(xform.cast(), "取事件生成器")?;
            DriverInner::Async(AsyncMft {
                xform: xform.clone(),
                event_gen,
                input_q: VecDeque::new(),
                need_input: false,
                drained: false,
                provides_samples: false,
                out_size: 0,
            })
        } else {
            DriverInner::Sync(SyncMft {
                xform: xform.clone(),
                provides_samples: false,
                out_size: 0,
            })
        };
        Ok((
            Driver {
                inner,
                ready: Vec::new(),
                stream_changed: false,
            },
            xform,
        ))
    }

    /// 类型协商完成后调用：刷新输出流信息 + 开流。
    fn begin_streaming(&mut self) -> AppResult<()> {
        unsafe {
            match &mut self.inner {
                DriverInner::Sync(s) => {
                    s.refresh_out_info()?;
                    hr(
                        s.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                        "开始流式处理",
                    )?;
                    // 同步 MFT 也补发 START_OF_STREAM：实测 h264_mf 不发这条
                    // 会把输入帧当不存在，只吐 SPS/PPS/AUD 不吐 slice。
                    hr(
                        s.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
                        "通知流开始",
                    )?;
                }
                DriverInner::Async(a) => {
                    let info = hr(a.xform.GetOutputStreamInfo(0), "读输出流信息")?;
                    a.provides_samples =
                        (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
                    a.out_size = info.cbSize;
                    hr(
                        a.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                        "开始流式处理",
                    )?;
                    hr(
                        a.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
                        "通知流开始",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// 喂一个输入样本，顺带收割已就绪的输出。
    fn push(&mut self, sample: IMFSample) -> AppResult<()> {
        match &mut self.inner {
            DriverInner::Sync(s) => unsafe {
                hr(s.xform.ProcessInput(0, &sample, 0), "ProcessInput")?;
                loop {
                    match s.pull_once()? {
                        Pulled::Sample(sm) => self.ready.push(sm),
                        Pulled::NeedMoreInput => break,
                        Pulled::StreamChange => {
                            self.stream_changed = true;
                            break;
                        }
                    }
                }
                Ok(())
            },
            DriverInner::Async(a) => {
                a.input_q.push_back(sample);
                a.poll(&mut self.ready)
            }
        }
    }

    /// 只收割输出，不喂输入（类型重谈后 / 收尾后用）。
    fn harvest(&mut self) -> AppResult<()> {
        match &mut self.inner {
            DriverInner::Sync(s) => {
                loop {
                    match s.pull_once()? {
                        Pulled::Sample(sm) => self.ready.push(sm),
                        Pulled::NeedMoreInput => break,
                        Pulled::StreamChange => {
                            self.stream_changed = true;
                            break;
                        }
                    }
                }
                Ok(())
            }
            DriverInner::Async(a) => a.poll(&mut self.ready),
        }
    }

    /// 结束输入并冲出全部剩余输出。
    fn finish(&mut self) -> AppResult<()> {
        match &mut self.inner {
            DriverInner::Sync(s) => unsafe {
                hr(
                    s.xform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0),
                    "通知流结束",
                )?;
                hr(
                    s.xform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0),
                    "命令排空",
                )?;
                loop {
                    match s.pull_once()? {
                        Pulled::Sample(sm) => self.ready.push(sm),
                        Pulled::NeedMoreInput => break,
                        Pulled::StreamChange => {
                            self.stream_changed = true;
                            break;
                        }
                    }
                }
                Ok(())
            },
            DriverInner::Async(a) => a.drain(&mut self.ready),
        }
    }

    fn take(&mut self) -> Vec<IMFSample> {
        std::mem::take(&mut self.ready)
    }
}

// ————————————————————————————————————————————————————————————
// 解码：HEVC MFT → NV12 系统内存
// ————————————————————————————————————————————————————————————

/// 一帧解码输出：连续 NV12 字节 + 自身 stride（别假设等于 width）。
struct Nv12Frame {
    data: Vec<u8>,
    stride: usize,
    width: usize,
    height: usize,
    pts_100ns: i64,
    dur_100ns: i64,
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
struct Decoder {
    reader: IMFSourceReader,
    width: usize,
    height: usize,
    stride: usize,
}

impl Decoder {
    fn new(input: &std::path::Path, width: usize, height: usize) -> AppResult<Self> {
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
    fn next_frame(&mut self) -> AppResult<Option<Nv12Frame>> {
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
fn frame_size_of(t: &IMFMediaType) -> AppResult<(usize, usize)> {
    let v = unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE) }
        .map_err(|e| AppError::Media(format!("Media Foundation 读帧尺寸失败（{e}）")))?;
    Ok(((v >> 32) as usize, (v & 0xffff_ffff) as usize))
}

// ————————————————————————————————————————————————————————————
// 缩放：Video Processor MFT（同步、NV12→NV12）
// ————————————————————————————————————————————————————————————

struct Scaler {
    mft: SyncMft,
    width: usize,
    height: usize,
    stride: usize,
}

impl Scaler {
    fn new(
        src_w: usize,
        src_h: usize,
        src_stride: usize,
        dst_w: usize,
        dst_h: usize,
        fps_num: u32,
        fps_den: u32,
    ) -> AppResult<Self> {
        unsafe {
            let xform: IMFTransform = hr(
                CoCreateInstance(&CLSID_VideoProcessorMFT, None, CLSCTX_ALL),
                "建 Video Processor",
            )?;
            let in_t = video_type(MFVideoFormat_NV12)?;
            set_size_rate(&in_t, src_w as u32, src_h as u32, fps_num, fps_den)?;
            hr(
                in_t.SetUINT32(&MF_MT_DEFAULT_STRIDE, src_stride as u32),
                "设输入行距",
            )?;
            hr(xform.SetInputType(0, &in_t, 0), "设缩放输入类型")?;
            let out_t = video_type(MFVideoFormat_NV12)?;
            set_size_rate(&out_t, dst_w as u32, dst_h as u32, fps_num, fps_den)?;
            hr(xform.SetOutputType(0, &out_t, 0), "设缩放输出类型")?;
            let mut mft = SyncMft {
                xform,
                provides_samples: false,
                out_size: 0,
            };
            mft.refresh_out_info()?;
            hr(
                mft.xform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                "开始缩放流",
            )?;
            let cur = hr(mft.xform.GetOutputCurrentType(0), "读缩放输出类型")?;
            let stride = type_stride(&cur, dst_w);
            Ok(Scaler {
                mft,
                width: dst_w,
                height: dst_h,
                stride,
            })
        }
    }

    fn scale(&mut self, frame: Nv12Frame) -> AppResult<Nv12Frame> {
        let sample = nv12_sample(
            &frame.data,
            frame.stride,
            frame.width,
            frame.height,
            frame.stride,
            frame.pts_100ns,
            frame.dur_100ns,
        )?;
        unsafe {
            hr(self.mft.xform.ProcessInput(0, &sample, 0), "缩放进帧")?;
        }
        let mut out = Vec::new();
        loop {
            match self.mft.pull_once()? {
                Pulled::Sample(s) => out.push(s),
                Pulled::NeedMoreInput => break,
                Pulled::StreamChange => {
                    self.mft.refresh_out_info()?;
                }
            }
        }
        let mut frames = out
            .into_iter()
            .filter_map(|s| {
                let (data, t, d) = sample_to_bytes(&s).ok()?;
                Some(Nv12Frame {
                    data,
                    stride: self.stride,
                    width: self.width,
                    height: self.height,
                    pts_100ns: t,
                    dur_100ns: d,
                })
            })
            .collect::<Vec<_>>();
        match frames.pop() {
            Some(f) => Ok(f),
            None => Err(AppError::Media("Video Processor 没有产出缩放帧".into())),
        }
    }
}

// ————————————————————————————————————————————————————————————
// 编码：H.264 MFT（硬件优先 / 测试回落软件）
// ————————————————————————————————————————————————————————————

#[derive(Clone, Copy, PartialEq, Debug)]
enum EncoderKind {
    /// 硬件 MFT（`MFT_ENUM_FLAG_HARDWARE`），探测验证过才用
    Hardware,
    /// in-box 软件编码器（h264_mf，同步 MFT）——测试通道
    #[cfg(test)]
    Software,
}

struct Encoder {
    driver: Driver,
    stride: usize,
    /// SPS/PPS（Annex-B），关键帧前插用（拼接器重建 moov 时样本描述取自
    /// 第 1 集，后续各集码流必须自带参数集）。编码器把它放在输出类型的
    /// `MF_MT_MPEG_SEQUENCE_HEADER` 或首个输出样本里，两处都试。
    param_sets: Option<Vec<u8>>,
    out_type: IMFMediaType,
    /// 每次喂帧记录的显示时间（秒），输出按到达顺序配对取用——
    /// 实测 h264_mf 会无视输入时间戳、按自己的 30fps 假设重打输出时间，
    /// NVENC 则原样保留；不信任回传值，统一用喂入值。
    fed_pts: VecDeque<f64>,
}

impl Encoder {
    fn new(
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
        kind: EncoderKind,
    ) -> AppResult<Self> {
        let flags = match kind {
            EncoderKind::Hardware => {
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER
            }
            // 没有 SOFTWARE 标志：软件 MFT = SYNCMFT（不带 HARDWARE）
            #[cfg(test)]
            EncoderKind::Software => MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
        };
        let activaes = enum_activates(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&type_info(MFMediaType_Video, MFVideoFormat_NV12)),
            Some(&type_info(MFMediaType_Video, MFVideoFormat_H264)),
        )?;
        let label = match kind {
            EncoderKind::Hardware => "硬件",
            #[cfg(test)]
            EncoderKind::Software => "软件",
        };
        let mut last = AppError::Media(format!("没有可用的{label} H.264 编码 MFT"));
        for a in &activaes {
            match Self::try_build(a, width, height, fps_num, fps_den, bitrate) {
                Ok(e) => return Ok(e),
                Err(e) => {
                    log::debug!("[Platform/mf] {label}编码 MFT 不可用，试下一个: {e}");
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn try_build(
        a: &IMFActivate,
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
    ) -> AppResult<Encoder> {
        // 类型协商顺序：硬件编码 MFT 普遍要求**先设输出类型再设输入**
        // （NVIDIA 的 MFT 先设输入直接回 MF_E_INVALIDTYPE 0xC00D6D60），
        // in-box 软件编码器两种顺序都收。两种顺序各试一遍。
        let mut last = AppError::Media("编码 MFT 类型协商失败".into());
        for output_first in [true, false] {
            match Self::negotiate_types(a, width, height, fps_num, fps_den, bitrate, output_first) {
                Ok(e) => return Ok(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn negotiate_types(
        a: &IMFActivate,
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
        output_first: bool,
    ) -> AppResult<Encoder> {
        let (mut driver, xform) = Driver::new(a)?;
        let build_input = |xform: &IMFTransform| -> AppResult<()> {
            unsafe {
                // 输入类型：优先用 MFT 列出的 NV12 模板（保住它自带的属性），
                // 帧尺寸/帧率/行距由我们覆盖；列不出再自建。
                let mut offered = None;
                let mut i = 0u32;
                loop {
                    let t = match xform.GetInputAvailableType(0, i) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    if t.GetGUID(&MF_MT_SUBTYPE).map(|g| g == MFVideoFormat_NV12) == Ok(true) {
                        offered = Some(t);
                        break;
                    }
                    i += 1;
                }
                let t = match offered {
                    Some(offered) => {
                        let fresh = hr(MFCreateMediaType(), "建媒体类型")?;
                        hr(offered.CopyAllItems(&fresh), "拷贝输入类型")?;
                        fresh
                    }
                    None => video_type(MFVideoFormat_NV12)?,
                };
                set_size_rate(&t, width as u32, height as u32, fps_num, fps_den)?;
                let _ = t.SetUINT32(&MF_MT_DEFAULT_STRIDE, width as u32);
                hr(xform.SetInputType(0, &t, 0), "设编码输入类型")
            }
        };
        let build_output = |xform: &IMFTransform| -> AppResult<()> {
            // 码控走 ICodecAPI（对齐 ffmpeg mfenc 的配置面）：不设的话
            // h264_mf 处于不可用的默认码控状态——实测只吐 SPS/PPS/AUD、
            // 一帧 slice 都不编。全部尽力而为，硬要求仍只有媒体类型上的码率。
            set_codecapi_u32(
                xform,
                &CODECAPI_AVEncCommonRateControlMode,
                eAVEncCommonRateControlMode_CBR.0 as u32,
                "码控模式 CBR",
            );
            set_codecapi_u32(
                xform,
                &CODECAPI_AVEncCommonMeanBitRate,
                bitrate.max(1) as u32,
                "均值码率",
            );
            set_codecapi_u32(xform, &CODECAPI_AVEncMPVGOPSize, 250, "GOP");
            set_codecapi_u32(xform, &CODECAPI_AVEncMPVDefaultBPictureCount, 0, "关 B 帧");
            set_codecapi_u32(xform, &CODECAPI_AVEncCommonRealTime, 1, "实时码控");
            unsafe {
                // 输出类型：H.264 + 码率。只有码率是硬性要求（码控失效=质量
                // 目标失效，对齐 vt 侧的分档）；profile 与 GOP 间隔尽力而为。
                let o = video_type(MFVideoFormat_H264)?;
                set_size_rate(&o, width as u32, height as u32, fps_num, fps_den)?;
                hr(
                    o.SetUINT32(&MF_MT_AVG_BITRATE, bitrate.max(1) as u32),
                    "设码率",
                )?;
                let _ = o.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32);
                let _ = o.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, 250);
                hr(xform.SetOutputType(0, &o, 0), "设编码输出类型")
            }
        };
        if output_first {
            build_output(&xform)?;
            build_input(&xform)?;
        } else {
            build_input(&xform)?;
            build_output(&xform)?;
        }
        driver.begin_streaming()?;
        let in_cur = hr(unsafe { xform.GetInputCurrentType(0) }, "读编码输入类型")?;
        let stride = type_stride(&in_cur, width);
        let out_type = hr(unsafe { xform.GetOutputCurrentType(0) }, "读编码输出类型")?;
        let param_sets = read_type_param_sets(&out_type);
        Ok(Encoder {
            driver,
            stride,
            param_sets,
            out_type,
            fed_pts: VecDeque::new(),
        })
    }

    /// 编一帧（显示时间戳由调用方按源时间轴给）。
    fn encode(&mut self, frame: Nv12Frame, pts_secs: f64, dur_secs: f64) -> AppResult<()> {
        self.fed_pts.push_back(pts_secs);
        let sample = nv12_sample(
            &frame.data,
            frame.stride,
            frame.width,
            frame.height,
            self.stride,
            secs_to_hns(pts_secs),
            secs_to_hns(dur_secs),
        )?;
        self.driver.push(sample)
    }

    /// 收尾并冲出尾帧。
    fn finish(&mut self) -> AppResult<()> {
        self.driver.finish()
    }

    /// 收割编码输出（Annex-B 单元 + 是否关键帧 + 显示时间）。
    ///
    /// 显示时间取喂帧时记录的时间轴（按到达顺序配对）；配不上（异常多出
    /// 的输出）给 NaN，由上层递增兜底顶开。
    fn take_units(&mut self) -> AppResult<Vec<(f64, Vec<u8>, bool)>> {
        self.driver.harvest()?;
        let samples = self.driver.take();
        let mut units = Vec::with_capacity(samples.len());
        for s in samples {
            let (raw, t, _d) = sample_to_bytes(&s)?;
            let keyframe = sample_is_clean(&s);
            if self.param_sets.is_none() {
                // 类型里没有就重读一次（有的 MFT 首样本后才填序列头），
                // 再不行从这个样本里解析（两处形态都见过）
                let mut ps = read_type_param_sets(&self.out_type);
                if ps.is_none() {
                    ps = param_sets_annexb_or_none(&raw);
                }
                self.param_sets = ps;
            }
            // 编码器输出格式实测不统一：h264_mf 直接吐 Annex-B（起始码
            // 开头），别的可能给 AVCC——按单元嗅探，别把 Annex-B 当 AVCC
            // 转换（长度字段走读会错读起始码，把 slice 整段丢掉）。
            let mut annexb = if raw.starts_with(&[0, 0, 0, 1]) || raw.starts_with(&[0, 0, 1]) {
                raw.clone()
            } else {
                crate::media::hevc::to_annexb(&raw, 4)
            };
            // 关键帧（以及首个样本，无论标没标）前插 SPS/PPS
            let first = units.is_empty();
            if (keyframe || first)
                && let Some(ps) = self.param_sets.as_deref()
                && !ps.is_empty()
            {
                annexb.splice(0..0, ps.iter().copied());
            }
            let _ = t;
            let pts = self.fed_pts.pop_front().unwrap_or(f64::NAN);
            units.push((pts, annexb, keyframe));
        }
        Ok(units)
    }
}

/// 输出类型的 `MF_MT_MPEG_SEQUENCE_HEADER` 里取 SPS/PPS（Annex-B）。
/// 序列头可能是起始码形态，也可能是 4 字节长度前缀形态，两种都解析。
fn read_type_param_sets(t: &IMFMediaType) -> Option<Vec<u8>> {
    let len = unsafe { t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }.ok()? as usize;
    if len == 0 || len > 1024 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; len];
    unsafe { t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut buf, None) }.ok()?;
    let ps = param_sets_annexb(&buf);
    (!ps.is_empty()).then_some(ps)
}

fn param_sets_annexb_or_none(raw: &[u8]) -> Option<Vec<u8>> {
    let ps = param_sets_annexb(raw);
    (!ps.is_empty()).then_some(ps)
}

/// 从一段字节（Annex-B 或 AVCC 形态）里抽 SPS（NAL 7）/PPS（NAL 8），
/// 重新拼成规范 Annex-B。
fn param_sets_annexb(blob: &[u8]) -> Vec<u8> {
    let mut nalus = split_annexb(blob);
    if nalus.is_empty() {
        nalus = split_avcc(blob);
    }
    let mut out = Vec::new();
    for n in nalus {
        if !n.is_empty() && matches!(n[0] & 0x1f, 7 | 8) {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(n);
        }
    }
    out
}

fn split_annexb(mut b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let Some((_, data)) = find_start_code(b) else {
        return out;
    };
    b = &b[data..];
    loop {
        match find_start_code(b) {
            Some((s, e)) => {
                out.push(&b[..s]);
                b = &b[e..];
            }
            None => {
                out.push(b);
                break;
            }
        }
    }
    out.retain(|n| !n.is_empty());
    out
}

/// 返回 (起始码起点, NAL 数据起点)。
fn find_start_code(b: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 3 < b.len() {
        if b[i] == 0 && b[i + 1] == 0 {
            if b[i + 2] == 1 {
                return Some((i, i + 3));
            }
            if i + 3 < b.len() && b[i + 2] == 0 && b[i + 3] == 1 {
                return Some((i, i + 4));
            }
        }
        i += 1;
    }
    None
}

fn split_avcc(b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= b.len() {
        let len = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
        pos += 4;
        if len == 0 || pos + len > b.len() {
            break;
        }
        out.push(&b[pos..pos + len]);
        pos += len;
    }
    out
}

// ————————————————————————————————————————————————————————————
// 码率
// ————————————————————————————————————————————————————————————

/// 码率标定：与 vt 侧同一公式。「0.12 bit/像素/帧」≈ libx264 `-crf 23` 档
/// （1080p25 ≈ 6.2 Mbps）——2026-10-07 用 NVENC MFT + libvmaf 双源复核：
/// 中低复杂度（近真人剧）VMAF 98.0 略优于 crf23 的 97.1；高运动内容 CBR
/// 固定码率天然吃亏（0.1 系数时 86.5 vs 92.4），提到 0.12 收窄到 ~3 分。
/// 再偏差大时只调这里的系数。
fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * 0.12;
    bps.clamp(800_000.0, 12_000_000.0) as i32
}

// ————————————————————————————————————————————————————————————
// 编排（与 vt 侧逐段对应）
// ————————————————————————————————————————————————————————————

/// 下一帧的显示时间：按升序时间轴逐帧取，重复/回退按半帧顶开。
fn next_pts(sorted_pts: &[f64], cursor: &mut usize, last: &mut Option<f64>, frame_dur: f64) -> f64 {
    let mut p = sorted_pts
        .get(*cursor)
        .copied()
        .unwrap_or_else(|| last.unwrap_or(0.0) + frame_dur);
    *cursor += 1;
    if let Some(prev) = *last
        && p <= prev
    {
        p = prev + frame_dur * 0.5;
    }
    *last = Some(p);
    p
}

/// 一帧解码输出 →（缩放）→ 编码 → 收割产物。
#[expect(
    clippy::too_many_arguments,
    reason = "参数表与平台 C API（虚表/会话属性）一一对应，硬拆参数结构反而失真"
)]
fn process_frame(
    frame: Nv12Frame,
    kind: EncoderKind,
    scale_to: Option<(usize, usize)>,
    fps: f64,
    frame_dur: f64,
    sorted_pts: &[f64],
    pts_cursor: &mut usize,
    last_pts: &mut Option<f64>,
    out_last: &mut Option<f64>,
    scaler: &mut Option<Scaler>,
    encoder: &mut Option<Encoder>,
    out_dims: &mut Option<(usize, usize)>,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    // 首帧才知道真实输出分辨率，缩放器与编码会话延迟到这里建
    if encoder.is_none() {
        let (src_w, src_h) = (frame.width, frame.height);
        let (out_w, out_h) = match scale_to {
            Some((w, h)) if (w, h) != (src_w, src_h) => {
                *scaler = Some(Scaler::new(
                    src_w,
                    src_h,
                    frame.stride,
                    w,
                    h,
                    fps_rate(fps),
                    1,
                )?);
                (w, h)
            }
            Some((w, h)) => (w, h),
            None => (src_w, src_h),
        };
        *encoder = Some(Encoder::new(
            out_w,
            out_h,
            fps_rate(fps),
            1,
            bitrate_for(out_w, out_h, fps),
            kind,
        )?);
        *out_dims = Some((out_w, out_h));
    }
    let frame = match scaler.as_mut() {
        Some(s) => s.scale(frame)?,
        None => frame,
    };
    let encoder = encoder.as_mut().expect("上一行刚建好");
    let pts = next_pts(sorted_pts, pts_cursor, last_pts, frame_dur);
    encoder.encode(frame, pts, frame_dur)?;
    emit_outputs(encoder, out_last, frame_dur, units, on_progress)
}

/// 收割编码器已就绪的输出。显示时间来自喂帧时间轴的配对（本身递增）；
/// 配不上（NaN）或异常回退时按输出侧自己的游标半帧顶开——**不能**与
/// `next_pts` 的喂入游标混用（见 run() 里的注释）。
fn emit_outputs(
    encoder: &mut Encoder,
    out_last: &mut Option<f64>,
    frame_dur: f64,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    for (raw, annexb, keyframe) in encoder.take_units()? {
        let pts = match *out_last {
            Some(prev) if !raw.is_finite() || raw <= prev => prev + frame_dur * 0.5,
            None if !raw.is_finite() => 0.0,
            _ => raw,
        };
        *out_last = Some(pts);
        units.push((pts, annexb, keyframe));
        if let Some(cb) = on_progress {
            cb(pts);
        }
    }
    Ok(())
}

/// 帧率 → 有理数分子（分母固定 1）。MF 只把它当码控提示，精度无关紧要。
fn fps_rate(fps: f64) -> u32 {
    fps.round().max(1.0) as u32
}

fn run(
    req: &PlatformRequest<'_>,
    demuxed: &crate::media::demux::Demuxed,
    video: &TrackInfo,
    kind: EncoderKind,
) -> AppResult<()> {
    let started = Instant::now();

    // 1) 源时间轴。解码走 SourceReader（内部自带参数集装配），帧以显示序
    //    出，逐帧对位升序时间轴。
    let fps = video.average_framerate().unwrap_or(25.0);
    let frame_dur = 1.0 / fps;
    let mut sorted_pts = video.sample_pts();
    if sorted_pts.is_empty() {
        sorted_pts = (0..video.samples.len()).map(|i| i as f64 / fps).collect();
    } else {
        sorted_pts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));

    // 2) 解码 → （缩放）→ 编码 流水
    let mut decoder = Decoder::new(req.input, video.width as usize, video.height as usize)?;
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut scaler: Option<Scaler> = None;
    let mut encoder: Option<Encoder> = None;
    let mut out_dims: Option<(usize, usize)> = None;
    let mut pts_cursor = 0usize;
    // 喂入时间轴与输出时间轴各用各的游标：混用会让输出兜底拿「喂到第几帧」
    // 当基准，把首输出时间错顶（实测 h264_mf 缓冲 33 帧后一次性吐出时，
    // 首帧 0.08 被顶成 0.78）。
    let mut last_pts: Option<f64> = None;
    let mut out_last: Option<f64> = None;

    while let Some(f) = decoder.next_frame()? {
        process_frame(
            f,
            kind,
            scale_to,
            fps,
            frame_dur,
            &sorted_pts,
            &mut pts_cursor,
            &mut last_pts,
            &mut out_last,
            &mut scaler,
            &mut encoder,
            &mut out_dims,
            &mut units,
            req.on_progress,
        )?;
    }

    // 3) 冲出编码器尾帧
    let mut encoder = match encoder {
        Some(e) => e,
        None => return Err(AppError::Media("HEVC 解码没有产出任何帧".into())),
    };
    encoder.finish()?;
    emit_outputs(
        &mut encoder,
        &mut out_last,
        frame_dur,
        &mut units,
        req.on_progress,
    )?;

    if units.is_empty() {
        return Err(AppError::Media("平台硬编没有产出任何帧".into()));
    }

    // 4) 音轨直通 + 封装（与软解路径同一套代码，faststart）
    let (out_w, out_h) = out_dims.unwrap_or((video.width as usize, video.height as usize));
    crate::media::transcode::mux_with_audio(
        req.input, demuxed, req.output, &units, out_w, out_h, fps as f32,
    )?;

    log::info!(
        "[Platform/mf] HEVC→H.264 完成：{} 帧（{out_w}x{out_h}），耗时 {:?}",
        units.len(),
        started.elapsed()
    );
    Ok(())
}

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
            let d = crate::media::demux::demux_file(out).unwrap();
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
        let d = crate::media::demux::demux_file(&out3).unwrap();
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
