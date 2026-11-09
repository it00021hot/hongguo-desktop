//! 像素桥与 CF 小工具：CVPixelBuffer / CMSampleBuffer 的裸指针包装（跨回调通道搬运）、CF 字典/数字助手、IOSurface 背板属性，
//! 以及像素搬移会话（[`Transfer`]，缩放与 VideoRange → FullRange 区间转换共用）。
//! 注意：VT 回调 trampoline（decode_cb / encode_cb）的 refcon 裸指针指向 Decompressor / Compressor 内部的 `Box<Sender>` 队列，
//! 回调与所属类型必须同文件——分别在 decode.rs / encode.rs，不在本文件。

use std::os::raw::c_void;

use hongguo_common::{AppError, AppResult};

use super::ffi;

/// 解码输出的像素缓冲。回调里 retain，Drop 时释放。
#[repr(transparent)]
pub(super) struct Pb(pub(super) ffi::CVPixelBufferRef);
unsafe impl Send for Pb {}

impl Clone for Pb {
    fn clone(&self) -> Self {
        if self.0.is_null() {
            return Pb(std::ptr::null_mut());
        }
        unsafe { ffi::CFRetain(self.0 as ffi::CFTypeRef) };
        Pb(self.0)
    }
}

impl Pb {
    /// 把紧密 8-bit I420 写进新的 NV12 `CVPixelBuffer`。
    pub(super) unsafe fn from_i420(
        y: &[u8],
        u: &[u8],
        v: &[u8],
        width: usize,
        height: usize,
    ) -> AppResult<Self> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(AppError::Media(format!("帧尺寸无法编码: {width}x{height}")));
        }
        let chroma = width / 2 * height / 2;
        if y.len() < width * height || u.len() < chroma || v.len() < chroma {
            return Err(AppError::Media("I420 平面长度不足".into()));
        }
        unsafe {
            let mut raw: ffi::CVPixelBufferRef = std::ptr::null_mut();
            let attrs = iosurface_attrs();
            let status = ffi::CVPixelBufferCreate(
                ffi::kCFAllocatorDefault,
                width,
                height,
                ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                attrs,
                &mut raw,
            );
            ffi::CFRelease(attrs as ffi::CFTypeRef);
            os(status, "建 NV12 缓冲")?;
            let pb = Pb(raw);
            os(ffi::CVPixelBufferLockBaseAddress(pb.0, 0), "锁 NV12 缓冲")?;
            let copied = copy_i420(&pb, y, u, v, width, height);
            ffi::CVPixelBufferUnlockBaseAddress(pb.0, 0);
            copied?;
            Ok(pb)
        }
    }

    pub(super) fn dims(&self) -> (usize, usize) {
        unsafe {
            (
                ffi::CVPixelBufferGetWidth(self.0),
                ffi::CVPixelBufferGetHeight(self.0),
            )
        }
    }
}

unsafe fn copy_i420(
    pb: &Pb,
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    height: usize,
) -> AppResult<()> {
    unsafe {
        if ffi::CVPixelBufferGetPlaneCount(pb.0) < 2 {
            return Err(AppError::Media("NV12 缓冲不是双平面".into()));
        }
        copy_plane(pb, 0, y, width, height, 1)?;
        let uv = ffi::CVPixelBufferGetBaseAddressOfPlane(pb.0, 1) as *mut u8;
        if uv.is_null() {
            return Err(AppError::Media("NV12 色度平面为空".into()));
        }
        let stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb.0, 1);
        let cw = width / 2;
        let ch = height / 2;
        for row in 0..ch {
            let dst = uv.add(row * stride);
            for col in 0..cw {
                *dst.add(col * 2) = u[row * cw + col];
                *dst.add(col * 2 + 1) = v[row * cw + col];
            }
        }
    }
    Ok(())
}

unsafe fn copy_plane(
    pb: &Pb,
    plane: usize,
    src: &[u8],
    width: usize,
    height: usize,
    bytes_per_sample: usize,
) -> AppResult<()> {
    unsafe {
        let base = ffi::CVPixelBufferGetBaseAddressOfPlane(pb.0, plane) as *mut u8;
        if base.is_null() {
            return Err(AppError::Media("NV12 亮度平面为空".into()));
        }
        let stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb.0, plane);
        let row_bytes = width * bytes_per_sample;
        for row in 0..height {
            std::ptr::copy_nonoverlapping(
                src.as_ptr().add(row * row_bytes),
                base.add(row * stride),
                row_bytes,
            );
        }
    }
    Ok(())
}

impl Drop for Pb {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ffi::CFRelease(self.0) };
        }
    }
}

/// 编码输出的样本缓冲。回调里 retain，处理完释放。
#[repr(transparent)]
pub(super) struct CmSb(pub(super) ffi::CMSampleBufferRef);
unsafe impl Send for CmSb {}

impl Drop for CmSb {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ffi::CFRelease(self.0) };
        }
    }
}

pub(super) fn os(status: ffi::OSStatus, what: &str) -> AppResult<()> {
    if status == ffi::noErr {
        Ok(())
    } else {
        Err(AppError::Media(format!(
            "VideoToolbox {what} 失败（OSStatus {status}）"
        )))
    }
}

/// 建一个小字典（键值都是借来的 CF 引用）。
pub(super) unsafe fn make_dict(
    entries: &[(ffi::CFTypeRef, ffi::CFTypeRef)],
) -> ffi::CFMutableDictionaryRef {
    unsafe {
        let dict = ffi::CFDictionaryCreateMutable(
            ffi::kCFAllocatorDefault,
            entries.len() as isize,
            ffi::kCFTypeDictionaryKeyCallBacks.as_ptr() as *const c_void,
            ffi::kCFTypeDictionaryValueCallBacks.as_ptr() as *const c_void,
        );
        for (k, v) in entries {
            ffi::CFDictionarySetValue(dict, *k, *v);
        }
        dict
    }
}

pub(super) unsafe fn make_int(v: i32) -> ffi::CFNumberRef {
    unsafe {
        ffi::CFNumberCreate(
            ffi::kCFAllocatorDefault,
            ffi::kCFNumberSInt32Type,
            &v as *const i32 as *const c_void,
        )
    }
}

/// IOSurface 背板声明：让解码输出/缩放目标都是 GPU 可共享的像素缓冲
pub(super) unsafe fn iosurface_attrs() -> ffi::CFMutableDictionaryRef {
    unsafe {
        let empty = empty_dict();
        let attrs = make_dict(&[(
            ffi::kCVPixelBufferIOSurfacePropertiesKey as ffi::CFTypeRef,
            empty as ffi::CFTypeRef,
        )]);
        ffi::CFRelease(empty as ffi::CFTypeRef);
        attrs
    }
}

/// 空 CF 字典（`kCVPixelBufferIOSurfacePropertiesKey` 的取值形态）。
pub(super) unsafe fn empty_dict() -> ffi::CFMutableDictionaryRef {
    unsafe { make_dict(&[]) }
}

pub(super) struct Transfer {
    session: ffi::VTPixelTransferSessionRef,
    width: usize,
    height: usize,
}

impl Drop for Transfer {
    fn drop(&mut self) {
        unsafe { ffi::CFRelease(self.session as ffi::CFTypeRef) };
    }
}

impl Transfer {
    pub(super) unsafe fn new(width: usize, height: usize) -> AppResult<Self> {
        unsafe {
            let mut session: ffi::VTPixelTransferSessionRef = std::ptr::null_mut();
            os(
                ffi::VTPixelTransferSessionCreate(ffi::kCFAllocatorDefault, &mut session),
                "建像素搬移会话",
            )?;
            Ok(Transfer {
                session,
                width,
                height,
            })
        }
    }

    /// 搬到目标分辨率/格式的缓冲（NV12 → NV12，GPU 上完成；区间转换
    /// VideoRange → FullRange 也由会话承担）。
    pub(super) unsafe fn transfer_to(&self, src: &Pb, pixel_format: ffi::OSType) -> AppResult<Pb> {
        unsafe {
            let mut dst: ffi::CVPixelBufferRef = std::ptr::null_mut();
            let attrs = iosurface_attrs();
            let st = ffi::CVPixelBufferCreate(
                ffi::kCFAllocatorDefault,
                self.width,
                self.height,
                pixel_format,
                attrs,
                &mut dst,
            );
            ffi::CFRelease(attrs as ffi::CFTypeRef);
            os(st, "建搬移目标缓冲")?;
            let dst = Pb(dst);
            os(
                ffi::VTPixelTransferSessionTransferImage(self.session, src.0, dst.0),
                "搬移帧",
            )?;
            Ok(dst)
        }
    }

    /// 缩到目标分辨率（转码链既有路径，'420v'）。
    pub(super) unsafe fn scale(&self, src: &Pb) -> AppResult<Pb> {
        unsafe { self.transfer_to(src, ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange) }
    }
}
