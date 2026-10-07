//! VideoToolbox / CoreMedia / CoreVideo 的最小 FFI 声明。
//!
//! 只声明转码链路用到的符号。VideoToolbox 是纯 C API（CF 对象），
//! 不需要 ObjC 运行时，手写声明比引 objc2 系省掉一整套版本依赖。
//! `#[link(kind = "dylib")]` 在 Apple 目标上会以 `-framework` 链接。
//!
//! 字段/签名以 macOS SDK 头文件为准（VTCompressionSession.h、
//! VTDecompressionSession.h、CMSampleBuffer.h、CVPixelBuffer.h）。
#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]

use std::os::raw::c_void;

pub type CFTypeRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFMutableDictionaryRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFBooleanRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFIndex = isize;
pub type OSType = u32;
pub type OSStatus = i32;
pub type Boolean = u8;

// —— 会话与对象引用（全部按不透明 CF 对象处理）——
pub type VTSessionRef = *mut c_void;
pub type VTDecompressionSessionRef = VTSessionRef;
pub type VTCompressionSessionRef = VTSessionRef;
pub type VTPixelTransferSessionRef = *mut c_void;
pub type CMVideoFormatDescriptionRef = *mut c_void;
pub type CMSampleBufferRef = *mut c_void;
pub type CMBlockBufferRef = *mut c_void;
pub type CVPixelBufferRef = *mut c_void;
pub type CMItemCount = isize;

/// `CMSampleTimingInfo`（CMSampleBuffer.h）。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CMSampleTimingInfo {
    pub duration: CMTime,
    pub presentationTimeStamp: CMTime,
    pub decodeTimeStamp: CMTime,
}

/// `CMTime`（CoreMedia.h）。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CMTime {
    pub value: i64,
    pub timescale: i32,
    pub flags: u32,
    pub epoch: i64,
}

pub type VTDecompressionOutputCallback = unsafe extern "C" fn(
    decompressionOutputRefCon: *mut c_void,
    sourceFrameRefCon: *mut c_void,
    status: OSStatus,
    infoFlags: u32,
    imageBuffer: CVPixelBufferRef,
    presentationTimeStamp: CMTime,
    presentationDuration: CMTime,
);

/// `VTDecompressionOutputCallbackRecord`。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VTDecompressionOutputCallbackRecord {
    pub callback: Option<VTDecompressionOutputCallback>,
    pub refcon: *mut c_void,
}

pub type VTCompressionOutputCallback = unsafe extern "C" fn(
    outputCallbackRefCon: *mut c_void,
    sourceFrameRefCon: *mut c_void,
    status: OSStatus,
    infoFlags: u32,
    sampleBuffer: CMSampleBufferRef,
);

// —— 常量 ——
pub const noErr: OSStatus = 0;
/// `kCMVideoCodecType_H264`（'avc1'）
pub const kCMVideoCodecType_H264: OSType = 0x6176_6331;
/// `kCMVideoCodecType_HEVC`（'hvc1'）
pub const kCMVideoCodecType_HEVC: OSType = 0x6876_6331;
/// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`（'420v'，NV12）
pub const kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange: OSType = 0x3432_3076;
/// `kVTDecodeFrameFlags`：不设 EnableAsynchronousDecompression 即同步解码
pub const kVTDecodeFrame_Synchronous: u32 = 0;
/// `kCFNumberSInt32Type`
pub const kCFNumberSInt32Type: CFIndex = 3;
/// `kCFStringEncodingUTF8`
pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

// —— CoreFoundation ——
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFAllocatorDefault: CFAllocatorRef;
    pub static kCFBooleanTrue: CFBooleanRef;
    pub static kCFBooleanFalse: CFBooleanRef;
    /// `CFDictionaryKeyCallBacks` / `CFDictionaryValueCallBacks` 的默认值，
    /// 按不透明内存块声明（64 位下各 48 字节：version + 5 个函数指针）
    pub static kCFTypeDictionaryKeyCallBacks: [u8; 48];
    pub static kCFTypeDictionaryValueCallBacks: [u8; 48];

    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    pub fn CFBooleanGetValue(boolean: CFBooleanRef) -> Boolean;
    pub fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        numValues: CFIndex,
        keyCallBacks: *const c_void,
        valueCallBacks: *const c_void,
    ) -> CFDictionaryRef;
    pub fn CFDictionaryCreateMutable(
        allocator: CFAllocatorRef,
        capacity: CFIndex,
        keyCallBacks: *const c_void,
        valueCallBacks: *const c_void,
    ) -> CFMutableDictionaryRef;
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFDictionarySetValue(dict: CFMutableDictionaryRef, key: CFTypeRef, value: CFTypeRef);
    pub fn CFNumberCreate(
        allocator: CFAllocatorRef,
        theType: CFIndex,
        valuePtr: *const c_void,
    ) -> CFNumberRef;
}

// —— CoreMedia ——
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    pub static kCMTimeInvalid: CMTime;
    pub static kCMSampleAttachmentKey_NotSync: CFStringRef;

    pub fn CMTimeMake(value: i64, timescale: i32) -> CMTime;
    pub fn CMBlockBufferCreateWithMemoryBlock(
        structureAllocator: CFAllocatorRef,
        memoryBlock: *mut c_void,
        blockLength: usize,
        blockAllocator: CFAllocatorRef,
        customBlockSource: *const c_void,
        offsetToData: usize,
        dataLength: usize,
        flags: u32,
        newCMBlockBufferOut: *mut CMBlockBufferRef,
    ) -> OSStatus;
    pub fn CMBlockBufferReplaceDataBytes(
        sourceBytes: *const c_void,
        destination: CMBlockBufferRef,
        offsetIntoDestination: usize,
        dataLength: usize,
    ) -> OSStatus;
    pub fn CMBlockBufferGetDataPointer(
        theBuffer: CMBlockBufferRef,
        atOffset: usize,
        lengthAtOffsetOut: *mut usize,
        totalLengthOut: *mut usize,
        dataPointerOut: *mut *mut u8,
    ) -> OSStatus;
    pub fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
        allocator: CFAllocatorRef,
        parameterSetCount: usize,
        parameterSetPointers: *const *const u8,
        parameterSetSizes: *const usize,
        NALUnitHeaderLength: i32,
        extensions: CFDictionaryRef,
        formatDescriptionOut: *mut CMVideoFormatDescriptionRef,
    ) -> OSStatus;
    /// H.264 描述里的参数集（SPS/PPS）。总数经 `parameterSetCountOut` 返回，
    /// 没有独立的 Count 函数。
    pub fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        videoDesc: CMVideoFormatDescriptionRef,
        parameterSetIndex: usize,
        parameterSetPointerOut: *mut *const u8,
        parameterSetSizeOut: *mut usize,
        parameterSetCountOut: *mut usize,
        NALUnitHeaderLengthOut: *mut i32,
    ) -> OSStatus;
    pub fn CMSampleBufferCreateReady(
        allocator: CFAllocatorRef,
        dataBuffer: CMBlockBufferRef,
        formatDescription: CMVideoFormatDescriptionRef,
        sampleCount: CMItemCount,
        numSampleTimingEntries: CMItemCount,
        sampleTimingArray: *const CMSampleTimingInfo,
        numSampleSizeEntries: CMItemCount,
        // `const size_t *`——曾经错写成 u32，API 按 8 字节读取直接越界
        sampleSizeArray: *const usize,
        sampleBufferOut: *mut CMSampleBufferRef,
    ) -> OSStatus;
    pub fn CMSampleBufferGetDataBuffer(sbuf: CMSampleBufferRef) -> CMBlockBufferRef;
    pub fn CMSampleBufferGetFormatDescription(
        sbuf: CMSampleBufferRef,
    ) -> CMVideoFormatDescriptionRef;
    pub fn CMSampleBufferGetNumSamples(sbuf: CMSampleBufferRef) -> CMItemCount;
    /// 编码器保留了我们传入的显示时间戳，这里原样读回
    pub fn CMSampleBufferGetOutputPresentationTimeStamp(sbuf: CMSampleBufferRef) -> CMTime;
    pub fn CMSampleBufferGetSampleAttachmentsArray(
        sbuf: CMSampleBufferRef,
        createIfNecessary: Boolean,
    ) -> CFArrayRef;
}

// —— CoreVideo ——
#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    pub static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    pub static kCVPixelBufferWidthKey: CFStringRef;
    pub static kCVPixelBufferHeightKey: CFStringRef;
    pub static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;

    pub fn CVPixelBufferCreate(
        allocator: CFAllocatorRef,
        width: usize,
        height: usize,
        pixelFormatType: OSType,
        pixelBufferAttributes: CFDictionaryRef,
        pixelBufferOut: *mut CVPixelBufferRef,
    ) -> OSStatus;
    pub fn CVPixelBufferGetWidth(pixelBuffer: CVPixelBufferRef) -> usize;
    pub fn CVPixelBufferGetHeight(pixelBuffer: CVPixelBufferRef) -> usize;
    pub fn CVPixelBufferLockBaseAddress(pixelBuffer: CVPixelBufferRef, flags: u32) -> OSStatus;
    pub fn CVPixelBufferUnlockBaseAddress(pixelBuffer: CVPixelBufferRef, flags: u32) -> OSStatus;
    pub fn CVPixelBufferGetBaseAddress(pixelBuffer: CVPixelBufferRef) -> *mut c_void;
    pub fn CVPixelBufferGetBytesPerRow(pixelBuffer: CVPixelBufferRef) -> usize;
}

// —— VideoToolbox ——
#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    pub static kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: CFStringRef;
    pub static kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder: CFStringRef;
    pub static kVTCompressionPropertyKey_AverageBitRate: CFStringRef;
    pub static kVTCompressionPropertyKey_MaxKeyFrameInterval: CFStringRef;
    pub static kVTCompressionPropertyKey_MaxFrameDelayCount: CFStringRef;
    pub static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    pub static kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder: CFStringRef;
    pub static kVTCompressionPropertyKey_RealTime: CFStringRef;
    pub static kVTProfileLevel_H264_High_AutoLevel: CFStringRef;

    pub fn VTSessionCopyProperty(
        session: VTSessionRef,
        propertyKey: CFStringRef,
        allocator: CFAllocatorRef,
        propertyValueOut: *mut CFTypeRef,
    ) -> OSStatus;
    pub fn VTSessionSetProperty(
        session: VTSessionRef,
        propertyKey: CFStringRef,
        propertyValue: CFTypeRef,
    ) -> OSStatus;
    pub fn VTCompressionSessionCreate(
        allocator: CFAllocatorRef,
        width: i32,
        height: i32,
        codecType: OSType,
        encoderSpecification: CFDictionaryRef,
        sourceImageBufferAttributes: CFDictionaryRef,
        compressedDataAllocator: CFAllocatorRef,
        outputCallback: Option<VTCompressionOutputCallback>,
        outputCallbackRefCon: *mut c_void,
        compressionSessionOut: *mut VTCompressionSessionRef,
    ) -> OSStatus;
    pub fn VTCompressionSessionEncodeFrame(
        session: VTCompressionSessionRef,
        imageBuffer: CVPixelBufferRef,
        presentationTimeStamp: CMTime,
        presentationDuration: CMTime,
        frameProperties: CFDictionaryRef,
        sourceFrameRefCon: *mut c_void,
        infoFlagsOut: *mut u32,
    ) -> OSStatus;
    pub fn VTCompressionSessionCompleteFrames(
        session: VTCompressionSessionRef,
        completeUntilPresentationTimeStamp: CMTime,
    ) -> OSStatus;
    pub fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);

    pub fn VTDecompressionSessionCreate(
        allocator: CFAllocatorRef,
        videoFormatDescription: CMVideoFormatDescriptionRef,
        videoDecoderSpecification: CFDictionaryRef,
        destinationImageBufferAttributes: CFDictionaryRef,
        outputCallback: *const VTDecompressionOutputCallbackRecord,
        decompressionSessionOut: *mut VTDecompressionSessionRef,
    ) -> OSStatus;
    pub fn VTDecompressionSessionDecodeFrame(
        session: VTDecompressionSessionRef,
        sampleBuffer: CMSampleBufferRef,
        decodeFlags: u32,
        // 每帧的 refCon，原样出现在输出回调的 sourceFrameRefCon 里。
        // 曾经漏掉这个参数：调用方的 infoFlagsOut 被它吃掉，真正的
        // infoFlagsOut 读到栈上垃圾，解码一启动就写野指针段错误。
        sourceFrameRefCon: *mut c_void,
        infoFlagsOut: *mut u32,
    ) -> OSStatus;
    pub fn VTDecompressionSessionFinishDelayedFrames(
        session: VTDecompressionSessionRef,
    ) -> OSStatus;
    pub fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);

    pub fn VTPixelTransferSessionCreate(
        allocator: CFAllocatorRef,
        pixelTransferSessionOut: *mut VTPixelTransferSessionRef,
    ) -> OSStatus;
    pub fn VTPixelTransferSessionTransferImage(
        session: VTPixelTransferSessionRef,
        sourceBuffer: CVPixelBufferRef,
        destinationBuffer: CVPixelBufferRef,
    ) -> OSStatus;
}
