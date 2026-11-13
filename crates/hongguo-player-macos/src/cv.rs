//! 内置软解直播放的上屏原语：解码帧 → NV12 `CVPixelBuffer` → `CMSampleBuffer` → `AVSampleBufferDisplayLayer`。
//!
//! 与 `avf.rs` 同一口径：不引入新的绑定 crate，C 函数按名字声明、ObjC 方法用 `msg_send!` 直投。
//! 两类坑：
//! - `msg_send!` 在 debug 构建里会校验参数的 ObjC 类型编码，CoreMedia 的不透明指针必须带上它们
//!   真实的结构体名（`^{opaqueCMSampleBuffer=}` / `^{OpaqueCMTimebase=}`），写成 `*mut c_void` 会 panic；
//! - 色彩口径以附件形式挂在像素缓冲上，系统按它换算并做色彩管理——与系统播放器观感一致，
//!   不需要（也不应该）像 WebView 那条路一样在出帧时归一。

use std::ffi::c_void;

use hongguo_softdec::{DecodedFrame, FrameColor, Matrix, Nv12Planes, copy_to_nv12};
use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};

use super::avf::CmTime;

type CFTypeRef = *const c_void;
type CFStringRef = *const AnyObject;
type CVPixelBufferRef = *mut c_void;
type CVPixelBufferPoolRef = *mut c_void;
type CMFormatDescriptionRef = *mut c_void;

/// `CMSampleBufferRef` 指向的不透明结构。
#[repr(C)]
pub struct OpaqueSampleBuffer {
    _private: [u8; 0],
}

// SAFETY: 编码与 CoreMedia 头文件的 `struct opaqueCMSampleBuffer *` 一致。
unsafe impl RefEncode for OpaqueSampleBuffer {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
}

/// `CMTimebaseRef` 指向的不透明结构。
#[repr(C)]
pub struct OpaqueTimebase {
    _private: [u8; 0],
}

// SAFETY: 编码与 CoreMedia 头文件的 `struct OpaqueCMTimebase *` 一致。
unsafe impl RefEncode for OpaqueTimebase {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("OpaqueCMTimebase", &[]));
}

/// `CMSampleTimingInfo`。
#[repr(C)]
struct SampleTiming {
    duration: CmTime,
    presentation: CmTime,
    decode: CmTime,
}

/// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`（'420v'）。
const NV12_VIDEO_RANGE: u32 = 0x3432_3076;
/// `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`（'420f'）。
const NV12_FULL_RANGE: u32 = 0x3432_3066;
/// `kCVAttachmentMode_ShouldPropagate`。
const ATTACHMENT_PROPAGATE: u32 = 1;
/// 帧时间戳的刻度：90 kHz，与解码侧 `presentation_tick` 同一刻度。
const TIMESCALE: i32 = 90_000;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: CFTypeRef);
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    static kCVPixelBufferWidthKey: CFStringRef;
    static kCVPixelBufferHeightKey: CFStringRef;
    static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    static kCVImageBufferYCbCrMatrixKey: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_709_2: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_601_4: CFStringRef;
    static kCVImageBufferYCbCrMatrix_ITU_R_2020: CFStringRef;
    static kCVImageBufferColorPrimariesKey: CFStringRef;
    static kCVImageBufferColorPrimaries_ITU_R_709_2: CFStringRef;
    static kCVImageBufferColorPrimaries_SMPTE_C: CFStringRef;
    static kCVImageBufferColorPrimaries_ITU_R_2020: CFStringRef;
    static kCVImageBufferTransferFunctionKey: CFStringRef;
    static kCVImageBufferTransferFunction_ITU_R_709_2: CFStringRef;

    fn CVPixelBufferPoolCreate(
        allocator: CFTypeRef,
        pool_attributes: CFTypeRef,
        buffer_attributes: CFTypeRef,
        pool: *mut CVPixelBufferPoolRef,
    ) -> i32;
    fn CVPixelBufferPoolCreatePixelBuffer(
        allocator: CFTypeRef,
        pool: CVPixelBufferPoolRef,
        buffer: *mut CVPixelBufferRef,
    ) -> i32;
    fn CVPixelBufferLockBaseAddress(buffer: CVPixelBufferRef, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: CVPixelBufferRef, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddressOfPlane(buffer: CVPixelBufferRef, plane: usize) -> *mut u8;
    fn CVPixelBufferGetBytesPerRowOfPlane(buffer: CVPixelBufferRef, plane: usize) -> usize;
    fn CVPixelBufferGetHeightOfPlane(buffer: CVPixelBufferRef, plane: usize) -> usize;
    fn CVBufferSetAttachment(
        buffer: CVPixelBufferRef,
        key: CFStringRef,
        value: CFTypeRef,
        mode: u32,
    );
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMVideoFormatDescriptionCreateForImageBuffer(
        allocator: CFTypeRef,
        image: CVPixelBufferRef,
        description: *mut CMFormatDescriptionRef,
    ) -> i32;
    fn CMSampleBufferCreateReadyWithImageBuffer(
        allocator: CFTypeRef,
        image: CVPixelBufferRef,
        description: CMFormatDescriptionRef,
        timing: *const SampleTiming,
        sample: *mut *mut OpaqueSampleBuffer,
    ) -> i32;
    fn CMTimebaseGetTime(timebase: *mut OpaqueTimebase) -> CmTime;
    fn CMTimebaseGetRate(timebase: *mut OpaqueTimebase) -> f64;
}

/// 时间基当前的走速（暂停为 0）。
pub fn timebase_rate(timebase: *mut OpaqueTimebase) -> f64 {
    // SAFETY: 调用方保证时间基有效。
    unsafe { CMTimebaseGetRate(timebase) }
}

fn class(name: &std::ffi::CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("缺少 Objective-C 类: {name:?}"))
}

/// 一个带 +1 引用的 CF 对象，Drop 时释放。
struct Owned(*mut c_void);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: 构造方保证持有一份 +1 引用。
            unsafe { CFRelease(self.0) };
        }
    }
}

/// 交给显示层的一帧。Drop 时释放。
pub struct SampleBuffer(*mut OpaqueSampleBuffer);

// SAFETY: CMSampleBuffer 是不可变的 CF 对象，可在线程间转移。
unsafe impl Send for SampleBuffer {}

impl SampleBuffer {
    pub fn as_ptr(&self) -> *mut OpaqueSampleBuffer {
        self.0
    }
}

impl Drop for SampleBuffer {
    fn drop(&mut self) {
        // SAFETY: `CMSampleBufferCreateReadyWithImageBuffer` 返回 +1。
        unsafe { CFRelease(self.0.cast()) };
    }
}

/// 固定尺寸与范围的 NV12 像素缓冲池：显示层放手后缓冲回池复用，不逐帧新建 IOSurface。
pub struct FramePacker {
    pool: Owned,
    key: (usize, usize, bool),
    color: FrameColor,
}

// SAFETY: CVPixelBufferPool 是线程安全的 CF 对象；本结构只在一个解码线程上使用。
unsafe impl Send for FramePacker {}

impl FramePacker {
    /// 为 `width × height` 的帧建池。失败返回 `None`。
    pub fn new(width: usize, height: usize, color: FrameColor) -> Option<Self> {
        let format = if color.full_range {
            NV12_FULL_RANGE
        } else {
            NV12_VIDEO_RANGE
        };
        let attributes = buffer_attributes(width, height, format);
        let mut pool: CVPixelBufferPoolRef = std::ptr::null_mut();
        // SAFETY: 属性字典在调用期间有效（NSDictionary 与 CFDictionary 免费桥接）。
        let status = unsafe {
            CVPixelBufferPoolCreate(
                std::ptr::null(),
                std::ptr::null(),
                Retained::as_ptr(&attributes).cast(),
                &mut pool,
            )
        };
        if status != 0 || pool.is_null() {
            log::error!("[Direct] 建 NV12 缓冲池失败（{width}x{height}，CVReturn {status}）");
            return None;
        }
        Some(Self {
            pool: Owned(pool),
            key: (width, height, color.full_range),
            color,
        })
    }

    /// 这个池能不能装下这一帧（尺寸与范围都得一致）。
    pub fn fits(&self, frame: &DecodedFrame, color: FrameColor) -> bool {
        self.key == (frame.width, frame.height, color.full_range) && self.color == color
    }

    /// 把一帧包成带显示时间的 `CMSampleBuffer`。
    pub fn pack(&self, frame: &DecodedFrame, pts: f64, duration: f64) -> Option<SampleBuffer> {
        let mut raw: CVPixelBufferRef = std::ptr::null_mut();
        // SAFETY: 池有效；出参是局部变量。
        let status =
            unsafe { CVPixelBufferPoolCreatePixelBuffer(std::ptr::null(), self.pool.0, &mut raw) };
        if status != 0 || raw.is_null() {
            log::warn!("[Direct] 从池里取像素缓冲失败（CVReturn {status}）");
            return None;
        }
        let buffer = Owned(raw);
        if !fill(buffer.0, frame) {
            return None;
        }
        attach_color(buffer.0, self.color);
        let mut description: CMFormatDescriptionRef = std::ptr::null_mut();
        // SAFETY: 像素缓冲有效；出参是局部变量。
        let status = unsafe {
            CMVideoFormatDescriptionCreateForImageBuffer(
                std::ptr::null(),
                buffer.0,
                &mut description,
            )
        };
        if status != 0 || description.is_null() {
            log::warn!("[Direct] 建视频格式描述失败（OSStatus {status}）");
            return None;
        }
        let description = Owned(description);
        let timing = SampleTiming {
            duration: CmTime::ticks((duration * f64::from(TIMESCALE)).round() as i64, TIMESCALE),
            presentation: CmTime::ticks((pts * f64::from(TIMESCALE)).round() as i64, TIMESCALE),
            decode: CmTime::INVALID,
        };
        let mut sample: *mut OpaqueSampleBuffer = std::ptr::null_mut();
        // SAFETY: 三个输入对象都有效；timing 是局部结构；出参是局部变量。
        let status = unsafe {
            CMSampleBufferCreateReadyWithImageBuffer(
                std::ptr::null(),
                buffer.0,
                description.0,
                &timing,
                &mut sample,
            )
        };
        if status != 0 || sample.is_null() {
            log::warn!("[Direct] 建样本缓冲失败（OSStatus {status}）");
            return None;
        }
        Some(SampleBuffer(sample))
    }
}

/// 锁住像素缓冲、写入一帧、解锁。
fn fill(buffer: CVPixelBufferRef, frame: &DecodedFrame) -> bool {
    // SAFETY: 缓冲有效；锁/解锁配对；平面指针与长度来自 CoreVideo，只在锁内使用。
    unsafe {
        if CVPixelBufferLockBaseAddress(buffer, 0) != 0 {
            return false;
        }
        let luma = CVPixelBufferGetBaseAddressOfPlane(buffer, 0);
        let chroma = CVPixelBufferGetBaseAddressOfPlane(buffer, 1);
        let copied = !luma.is_null() && !chroma.is_null() && {
            let luma_stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, 0);
            let chroma_stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, 1);
            let luma_rows = CVPixelBufferGetHeightOfPlane(buffer, 0);
            let chroma_rows = CVPixelBufferGetHeightOfPlane(buffer, 1);
            copy_to_nv12(
                frame,
                Nv12Planes {
                    luma: std::slice::from_raw_parts_mut(luma, luma_stride * luma_rows),
                    luma_stride,
                    chroma: std::slice::from_raw_parts_mut(chroma, chroma_stride * chroma_rows),
                    chroma_stride,
                },
            )
        };
        CVPixelBufferUnlockBaseAddress(buffer, 0);
        copied
    }
}

/// 把 YUV 口径挂到像素缓冲上：矩阵、色域、传递函数。系统按它换算，并与系统播放器同样做色彩管理。
fn attach_color(buffer: CVPixelBufferRef, color: FrameColor) {
    // SAFETY: 键值都是 CoreVideo 导出的常量字符串；附件被缓冲 retain。
    unsafe {
        let (matrix, primaries) = match color.matrix {
            Matrix::Bt709 => (
                kCVImageBufferYCbCrMatrix_ITU_R_709_2,
                kCVImageBufferColorPrimaries_ITU_R_709_2,
            ),
            Matrix::Bt601 => (
                kCVImageBufferYCbCrMatrix_ITU_R_601_4,
                kCVImageBufferColorPrimaries_SMPTE_C,
            ),
            Matrix::Bt2020 => (
                kCVImageBufferYCbCrMatrix_ITU_R_2020,
                kCVImageBufferColorPrimaries_ITU_R_2020,
            ),
        };
        CVBufferSetAttachment(
            buffer,
            kCVImageBufferYCbCrMatrixKey,
            matrix.cast(),
            ATTACHMENT_PROPAGATE,
        );
        CVBufferSetAttachment(
            buffer,
            kCVImageBufferColorPrimariesKey,
            primaries.cast(),
            ATTACHMENT_PROPAGATE,
        );
        // SDR 内容的传递函数统一按 BT.709；HDR 不在支持范围内。
        CVBufferSetAttachment(
            buffer,
            kCVImageBufferTransferFunctionKey,
            kCVImageBufferTransferFunction_ITU_R_709_2.cast(),
            ATTACHMENT_PROPAGATE,
        );
    }
}

/// 像素缓冲属性：格式、尺寸、IOSurface 背板（显示层零拷贝合成的前提）。
fn buffer_attributes(width: usize, height: usize, format: u32) -> Retained<AnyObject> {
    // SAFETY: 公开类方法与实例方法；键是 CoreVideo 导出的常量（与 NSString 免费桥接）。
    unsafe {
        let dictionary: Retained<AnyObject> = msg_send![class(c"NSMutableDictionary"), dictionary];
        let number = |value: i64| -> Retained<AnyObject> {
            msg_send![class(c"NSNumber"), numberWithLongLong: value]
        };
        let surface: Retained<AnyObject> = msg_send![class(c"NSDictionary"), dictionary];
        let entries: [(CFStringRef, Retained<AnyObject>); 4] = [
            (kCVPixelBufferPixelFormatTypeKey, number(i64::from(format))),
            (kCVPixelBufferWidthKey, number(width as i64)),
            (kCVPixelBufferHeightKey, number(height as i64)),
            (kCVPixelBufferIOSurfacePropertiesKey, surface),
        ];
        for (key, value) in entries {
            let _: () = msg_send![&*dictionary, setObject: &*value, forKey: key];
        }
        dictionary
    }
}

// —— AVSampleBufferDisplayLayer ——

/// 新建一个视频显示层。
pub fn display_layer() -> Retained<AnyObject> {
    // SAFETY: 公开类的 +new。
    unsafe { msg_send![class(c"AVSampleBufferDisplayLayer"), new] }
}

/// 显示层还能不能再收帧（背压：它的内部队列满了就是 false）。
pub fn ready_for_more(layer: &AnyObject) -> bool {
    // SAFETY: 只读 BOOL 属性。
    unsafe { msg_send![layer, isReadyForMoreMediaData] }
}

pub fn enqueue(layer: &AnyObject, sample: &SampleBuffer) {
    // SAFETY: 样本缓冲有效；显示层 retain 它。
    let _: () = unsafe { msg_send![layer, enqueueSampleBuffer: sample.as_ptr()] };
}

/// 清掉排队中的帧（跳转后旧时间轴的帧不能再显示）。保留当前画面，避免闪黑。
pub fn flush(layer: &AnyObject) {
    // SAFETY: 公开实例方法。
    let _: () = unsafe { msg_send![layer, flush] };
}

/// 清掉排队中的帧连同当前画面。
pub fn flush_and_clear(layer: &AnyObject) {
    // SAFETY: 公开实例方法。
    let _: () = unsafe { msg_send![layer, flushAndRemoveImage] };
}

/// 显示层是否进了失败态（`AVQueuedSampleBufferRenderingStatusFailed` = 2），以及原因。
pub fn failure(layer: &AnyObject) -> Option<String> {
    // SAFETY: 只读属性；error 可空。
    unsafe {
        let status: isize = msg_send![layer, status];
        if status != 2 {
            return None;
        }
        let error: *mut AnyObject = msg_send![layer, error];
        if error.is_null() {
            return Some("显示层进入失败态".into());
        }
        let description: *mut AnyObject = msg_send![error, localizedDescription];
        let text: *mut objc2_foundation::NSString = description.cast();
        Some(if text.is_null() {
            "显示层进入失败态".into()
        } else {
            (*text).to_string()
        })
    }
}

/// 让显示层跟着这个时间基走（暂停、倍速、跳转都随之生效）。
pub fn set_control_timebase(layer: &AnyObject, timebase: *mut OpaqueTimebase) {
    // SAFETY: 时间基由 AVPlayerItem 持有；显示层 retain 它。
    let _: () = unsafe { msg_send![layer, setControlTimebase: timebase] };
}

/// 时间基的当前时间（秒）。
pub fn timebase_seconds(timebase: *mut OpaqueTimebase) -> f64 {
    // SAFETY: 调用方保证时间基有效。
    unsafe { CMTimebaseGetTime(timebase) }.seconds()
}
