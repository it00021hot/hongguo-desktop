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
    /// 把紧密 8-bit I420 写进新的 NV12 `CVPixelBuffer`。逐帧新建 IOSurface；
    /// 连续送帧走 [`Nv12Pool::nv12_of`]。
    pub(super) unsafe fn from_i420(
        y: &[u8],
        u: &[u8],
        v: &[u8],
        width: usize,
        height: usize,
    ) -> AppResult<Self> {
        check_i420(y, u, v, width, height)?;
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
            fill_i420(&pb, y, u, v, width, height)?;
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

fn check_i420(y: &[u8], u: &[u8], v: &[u8], width: usize, height: usize) -> AppResult<()> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(AppError::Media(format!("帧尺寸无法编码: {width}x{height}")));
    }
    let chroma = width / 2 * height / 2;
    if y.len() < width * height || u.len() < chroma || v.len() < chroma {
        return Err(AppError::Media("I420 平面长度不足".into()));
    }
    Ok(())
}

/// 锁住缓冲、写入 I420、解锁。调用方已用 [`check_i420`] 验过平面长度。
unsafe fn fill_i420(
    pb: &Pb,
    y: &[u8],
    u: &[u8],
    v: &[u8],
    width: usize,
    height: usize,
) -> AppResult<()> {
    unsafe {
        os(ffi::CVPixelBufferLockBaseAddress(pb.0, 0), "锁 NV12 缓冲")?;
        let copied = copy_i420(pb, y, u, v, width, height);
        ffi::CVPixelBufferUnlockBaseAddress(pb.0, 0);
        copied
    }
}

/// 按固定分辨率反复取 NV12 缓冲的池。逐帧 `CVPixelBufferCreate` 每次都新建并回收一块
/// IOSurface，分配开销比拷贝本身还大；池里的缓冲在编码器放手后自动回到池中。
pub(super) struct Nv12Pool {
    pool: ffi::CVPixelBufferPoolRef,
    width: usize,
    height: usize,
}

impl Drop for Nv12Pool {
    fn drop(&mut self) {
        unsafe { ffi::CFRelease(self.pool as ffi::CFTypeRef) };
    }
}

impl Nv12Pool {
    pub(super) unsafe fn new(width: usize, height: usize) -> AppResult<Self> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(AppError::Media(format!("帧尺寸无法编码: {width}x{height}")));
        }
        unsafe {
            let format = make_int(ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
            let w = make_int(width as i32);
            let h = make_int(height as i32);
            let surface = empty_dict();
            let attrs = make_dict(&[
                (
                    ffi::kCVPixelBufferPixelFormatTypeKey as ffi::CFTypeRef,
                    format as ffi::CFTypeRef,
                ),
                (
                    ffi::kCVPixelBufferWidthKey as ffi::CFTypeRef,
                    w as ffi::CFTypeRef,
                ),
                (
                    ffi::kCVPixelBufferHeightKey as ffi::CFTypeRef,
                    h as ffi::CFTypeRef,
                ),
                (
                    ffi::kCVPixelBufferIOSurfacePropertiesKey as ffi::CFTypeRef,
                    surface as ffi::CFTypeRef,
                ),
            ]);
            let mut pool: ffi::CVPixelBufferPoolRef = std::ptr::null_mut();
            let status = ffi::CVPixelBufferPoolCreate(
                ffi::kCFAllocatorDefault,
                std::ptr::null(),
                attrs as ffi::CFDictionaryRef,
                &mut pool,
            );
            for cf in [
                attrs as ffi::CFTypeRef,
                surface as ffi::CFTypeRef,
                h as ffi::CFTypeRef,
                w as ffi::CFTypeRef,
                format as ffi::CFTypeRef,
            ] {
                ffi::CFRelease(cf);
            }
            os(status, "建 NV12 缓冲池")?;
            Ok(Self {
                pool,
                width,
                height,
            })
        }
    }

    pub(super) fn dims(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// 从池里取一块缓冲并写入 I420。帧尺寸必须等于建池尺寸。
    pub(super) unsafe fn nv12_of(
        &self,
        y: &[u8],
        u: &[u8],
        v: &[u8],
        width: usize,
        height: usize,
    ) -> AppResult<Pb> {
        if (width, height) != (self.width, self.height) {
            return Err(AppError::Media(format!(
                "帧尺寸 {width}x{height} 与缓冲池 {}x{} 不符",
                self.width, self.height
            )));
        }
        check_i420(y, u, v, width, height)?;
        unsafe {
            let mut raw: ffi::CVPixelBufferRef = std::ptr::null_mut();
            let status = ffi::CVPixelBufferPoolCreatePixelBuffer(
                ffi::kCFAllocatorDefault,
                self.pool,
                &mut raw,
            );
            os(status, "从池里取 NV12 缓冲")?;
            let pb = Pb(raw);
            fill_i420(&pb, y, u, v, width, height)?;
            Ok(pb)
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
        if stride < cw * 2 || ffi::CVPixelBufferGetHeightOfPlane(pb.0, 1) < ch {
            return Err(AppError::Media("NV12 色度平面尺寸不足".into()));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 读回 NV12 缓冲：紧密亮度、紧密 U、紧密 V。
    unsafe fn read_back(pb: &Pb, width: usize, height: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        unsafe {
            assert_eq!(ffi::CVPixelBufferLockBaseAddress(pb.0, 0), ffi::noErr);
            let y_base = ffi::CVPixelBufferGetBaseAddressOfPlane(pb.0, 0) as *const u8;
            let y_stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb.0, 0);
            let uv_base = ffi::CVPixelBufferGetBaseAddressOfPlane(pb.0, 1) as *const u8;
            let uv_stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb.0, 1);
            let mut y = Vec::new();
            for row in 0..height {
                y.extend_from_slice(std::slice::from_raw_parts(
                    y_base.add(row * y_stride),
                    width,
                ));
            }
            let (mut u, mut v) = (Vec::new(), Vec::new());
            for row in 0..height / 2 {
                let line = std::slice::from_raw_parts(uv_base.add(row * uv_stride), width);
                for pair in line.as_chunks::<2>().0 {
                    u.push(pair[0]);
                    v.push(pair[1]);
                }
            }
            ffi::CVPixelBufferUnlockBaseAddress(pb.0, 0);
            (y, u, v)
        }
    }

    fn ramp(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    #[test]
    fn pooled_buffer_holds_the_i420_planes_interleaved() {
        // 宽度取 2 的倍数但不是 16/32 的倍数，覆盖向量化交织的收尾。
        let (width, height) = (70usize, 34usize);
        let y = ramp(width * height, 1);
        let u = ramp(width / 2 * height / 2, 77);
        let v = ramp(width / 2 * height / 2, 191);
        let pool = unsafe { Nv12Pool::new(width, height) }.expect("建池");
        let pb = unsafe { pool.nv12_of(&y, &u, &v, width, height) }.expect("取缓冲");
        assert_eq!(pb.dims(), (width, height));
        let (got_y, got_u, got_v) = unsafe { read_back(&pb, width, height) };
        assert_eq!(got_y, y);
        assert_eq!(got_u, u);
        assert_eq!(got_v, v);

        let fresh = unsafe { Pb::from_i420(&y, &u, &v, width, height) }.expect("逐帧新建");
        let (ref_y, ref_u, ref_v) = unsafe { read_back(&fresh, width, height) };
        assert_eq!(
            (got_y, got_u, got_v),
            (ref_y, ref_u, ref_v),
            "池路径与逐帧新建应一致"
        );
    }

    #[test]
    fn pool_recycles_a_released_buffer() {
        let (width, height) = (64usize, 64usize);
        let y = vec![16u8; width * height];
        let c = vec![128u8; width / 2 * height / 2];
        let pool = unsafe { Nv12Pool::new(width, height) }.expect("建池");
        let attrs = unsafe {
            let limit = make_int(1);
            let attrs = make_dict(&[(
                ffi::kCVPixelBufferPoolAllocationThresholdKey as ffi::CFTypeRef,
                limit as ffi::CFTypeRef,
            )]);
            ffi::CFRelease(limit as ffi::CFTypeRef);
            attrs
        };
        let take = || {
            let mut raw = std::ptr::null_mut();
            let status = unsafe {
                ffi::CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(
                    ffi::kCFAllocatorDefault,
                    pool.pool,
                    attrs,
                    &mut raw,
                )
            };
            if status == ffi::kCVReturnSuccess {
                Ok(Pb(raw))
            } else {
                Err(status)
            }
        };
        let first = unsafe { pool.nv12_of(&y, &c, &c, width, height) }.expect("第一块");
        let retained = first.clone();
        drop(first);
        assert_eq!(
            take().err(),
            Some(ffi::kCVReturnWouldExceedAllocationThreshold),
            "编码器仍持有缓冲时不能在单块预算内再次分配"
        );
        drop(retained);
        // CoreVideo 可以更换对象地址；限制分配数后成功取回才证明缓冲可复用。
        let second = take().expect("放手后应能在单块预算内复用缓冲");
        assert_eq!(second.dims(), (width, height));
        let next_y = vec![32u8; width * height];
        unsafe { fill_i420(&second, &next_y, &c, &c, width, height) }.expect("写入下一帧");
        assert_eq!(
            unsafe { read_back(&second, width, height) },
            (next_y, c.clone(), c)
        );
        unsafe { ffi::CFRelease(attrs as ffi::CFTypeRef) };
    }

    #[test]
    fn pool_rejects_other_sizes_and_short_planes() {
        let pool = unsafe { Nv12Pool::new(64, 64) }.expect("建池");
        let y = vec![0u8; 64 * 64];
        let c = vec![0u8; 32 * 32];
        assert!(unsafe { pool.nv12_of(&y, &c, &c, 32, 32) }.is_err());
        assert!(unsafe { pool.nv12_of(&y, &c[..10], &c, 64, 64) }.is_err());
        assert!(unsafe { Nv12Pool::new(63, 64) }.is_err());
    }
}
