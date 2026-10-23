//! 媒体类型与样本小工具：媒体类型构造（[`video_type`] / [`set_size_rate`] /
//! [`type_stride`]）、NV12 平面帧与裸字节 → IMFSample（两者共用
//! [`locked_buffer`] + [`finish_sample`] 建样本骨架，后者在 playback 侧）、
//! 样本 → 连续字节与关键帧判定、100ns 时间换算。

use windows::Win32::Media::MediaFoundation::*;

use hongguo_common::{AppError, AppResult};
use windows::core::GUID;

use super::mft::hr;

// ————————————————————————————————————————————————————————————
// 媒体类型与样本小工具
// ————————————————————————————————————————————————————————————

const HNS: f64 = 10_000_000.0;

pub(super) fn secs_to_hns(s: f64) -> i64 {
    (s * HNS).round() as i64
}

/// 一个空的视频媒体类型（major=Video + 指定 subtype）。
pub(super) fn video_type(subtype: GUID) -> AppResult<IMFMediaType> {
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
pub(super) fn set_size_rate(
    t: &IMFMediaType,
    w: u32,
    h: u32,
    fps_num: u32,
    fps_den: u32,
) -> AppResult<()> {
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
pub(super) fn type_stride(t: &IMFMediaType, width: usize) -> usize {
    unsafe { t.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
        .map(|v| v as usize)
        .unwrap_or(width)
}

/// 建样本骨架前半：`MFCreateSample` → 建定长缓冲 → `Lock`，返回
/// （样本, 缓冲, 可写指针）。拷贝完成后必须把同一对样本/缓冲交给
/// [`finish_sample`] 收尾（`Unlock` → `SetCurrentLength` → `AddBuffer` →
/// 时间戳/时长）。
///
/// SAFETY：`unsafe fn` 的调用方须保证返回的裸指针只在 `buf` 存活且未
/// `Unlock` 期间使用（即 `finish_sample` 之前）——两个约定由
/// [`nv12_sample`] 以固定顺序满足。
pub(super) unsafe fn locked_buffer(len: usize) -> AppResult<(IMFSample, IMFMediaBuffer, *mut u8)> {
    unsafe {
        let sample = hr(MFCreateSample(), "建样本")?;
        let buf = hr(MFCreateMemoryBuffer(len as u32), "建样本缓冲")?;
        let mut ptr = std::ptr::null_mut();
        hr(buf.Lock(&mut ptr, None, None), "锁样本缓冲")?;
        Ok((sample, buf, ptr))
    }
}

/// 建样本骨架后半：`Unlock` → `SetCurrentLength(len)` → `AddBuffer` →
/// `SetSampleTime` / `SetSampleDuration`。时间戳/时长为负（缺失）时跳过
/// 设置——读取侧（[`sample_to_bytes`]）对「未设置」与负值同样映射 -1 交给
/// 上层递增兜底，两条旧路径（无条件设置/条件设置）在此收敛。
pub(super) unsafe fn finish_sample(
    sample: &IMFSample,
    buf: &IMFMediaBuffer,
    len: usize,
    pts_100ns: i64,
    dur_100ns: i64,
) -> AppResult<()> {
    unsafe {
        let _ = buf.Unlock();
        hr(buf.SetCurrentLength(len as u32), "设样本长度")?;
        hr(sample.AddBuffer(buf), "挂样本缓冲")?;
        if pts_100ns >= 0 {
            hr(sample.SetSampleTime(pts_100ns), "设样本时间")?;
        }
        if dur_100ns >= 0 {
            hr(sample.SetSampleDuration(dur_100ns), "设样本时长")?;
        }
        Ok(())
    }
}

/// NV12 平面帧 → 样本（按目标 stride 逐行拷贝，不假设 stride=width）。
pub(super) fn nv12_sample(
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
        let (sample, buf, ptr) = locked_buffer(total)?;
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
        finish_sample(&sample, &buf, total, pts_100ns, dur_100ns)?;
        Ok(sample)
    }
}

/// 样本 → 连续字节（连续化 + Lock 拷贝）。返回 `(字节, 时间戳, 时长)`，
/// 时间戳缺失时为 -1（交给上层递增兜底）。
pub(super) fn sample_to_bytes(sample: &IMFSample) -> AppResult<(Vec<u8>, i64, i64)> {
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
pub(super) fn sample_is_clean(sample: &IMFSample) -> bool {
    unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0
}
