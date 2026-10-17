//! 像素桥与 CF 小工具：CVPixelBuffer / CMSampleBuffer 的裸指针包装（跨回调
//! 通道搬运）、CF 字典/数字助手、IOSurface 背板属性，以及像素搬移会话
//! （[`Transfer`]，缩放与 VideoRange → FullRange 区间转换共用）。
//!
//! 注意：VT 回调 trampoline（decode_cb / encode_cb）的 refcon 裸指针指向
//! Decompressor / Compressor 内部的 `Box<Sender>` 队列，回调与所属类型必须
//! 同文件——分别在 decode.rs / encode.rs，不在本文件。

use std::os::raw::c_void;

use hongguo_common::{AppError, AppResult};

use super::ffi;

// ————————————————————————————————————————————————————————————
// 裸指针包装：跨回调通道搬运 CF 对象
// ————————————————————————————————————————————————————————————

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
    pub(super) fn dims(&self) -> (usize, usize) {
        unsafe {
            (
                ffi::CVPixelBufferGetWidth(self.0),
                ffi::CVPixelBufferGetHeight(self.0),
            )
        }
    }
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

// ————————————————————————————————————————————————————————————
// CF 小工具
// ————————————————————————————————————————————————————————————

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

// ————————————————————————————————————————————————————————————
// 缩放
// ————————————————————————————————————————————————————————————

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
