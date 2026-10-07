//! macOS VideoToolbox 硬编后端。
//!
//! 链路与软解路径（`media::transcode`）逐段对应，只是「解码 → YUV → 编码」
//! 被整体换成了 GPU 上的「解码 → NV12 CVPixelBuffer → 编码」，IOSurface
//! 背板的像素缓冲在编解码之间零拷贝：
//!
//! ```text
//! MP4 样本（长度前缀 NAL，原样）─VTDecompressionSession→ NV12
//!     ─(VTPixelTransferSession 缩放，可选)→ VTCompressionSession
//!     → H.264 AVCC ─Annex-B 转换 + SPS/PPS 前插→ mux_h264 → MP4 out
//! ```
//!
//! 复用软解路径的部分：MP4 解复用、参数集读取（`hvcC`）、音轨 AAC 直通、
//! 封装（muxide faststart）、临时文件原子替换。产物与软解/ffmpeg 完全同构。
//!
//! 时间戳：逐帧显示时间来自源样本表（`stts`/`ctts` 展开），不再按固定帧率
//! 合成——换后端顺手消灭第一类音画漂移。
//!
//! 探测纪律：`RequireHardware` 下建 256×256 会话，建不出来就是真没有硬编。
//! 256×256 的下限与 ffmpeg 侧 nvenc 的教训一致（更小的探测帧会被最小分辨率
//! 拒掉，把可用的硬编误判成不可用）。

mod ffi;

use std::collections::VecDeque;
use std::os::raw::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

use parking_lot::RwLock;

use crate::domain::mp4::sample_table::TrackInfo;
use crate::error::{AppError, AppResult};

use super::PlatformRequest;

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
    Some(run(req, &demuxed, &video.info, HwPolicy::Require))
}

/// 仅测试构建：以「允许回落软件」的策略跑完整链路，让没有硬编的机器
/// 也能验证管线本身（解码/Annex-B/封装/时间戳）。硬件与否由探测测试另行验证。
#[cfg(test)]
pub fn transcode_h264_for_tests(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let demuxed = crate::media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !crate::media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run(req, &demuxed, &video.info, HwPolicy::Enable))
}

fn probe_h264_hw_encoder() -> bool {
    match unsafe { probe_reports_hardware() } {
        Ok(hw) => hw,
        Err(e) => {
            log::warn!("[Platform/vt] 平台硬编探测失败，按不可用处理: {e}");
            false
        }
    }
}

/// 探测纪律与 ffmpeg 侧同一套：「能建会话」不算数，编码器自己说「我走了
/// 硬件」才算。`RequireHardware` 在 Intel Mac 上常见 -12903 误报（明明有
/// QuickSync 却枚举不到），所以用 `Enable`（允许硬件、不强制）建会话，
/// **实编一帧**后读 `UsingHardwareAcceleratedVideoEncoder`。
unsafe fn probe_reports_hardware() -> AppResult<bool> {
    let mut c = Compressor::new(256, 256, 1_000_000, HwPolicy::Enable)?;
    let frame = make_test_frame(256)?;
    c.encode(&frame, 0.0, 1.0 / 30.0)?;
    c.finish()?;
    let outputs = c.take_outputs();
    drop(outputs);
    c.using_hardware()
}

/// 探测用的灰帧：NV12 平面铺 0x80（中性灰），避免未初始化内存进编码器。
unsafe fn make_test_frame(size: usize) -> AppResult<Pb> {
    let mut pb: ffi::CVPixelBufferRef = std::ptr::null_mut();
    let attrs = iosurface_attrs();
    let st = ffi::CVPixelBufferCreate(
        ffi::kCFAllocatorDefault,
        size,
        size,
        ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        attrs,
        &mut pb,
    );
    ffi::CFRelease(attrs as ffi::CFTypeRef);
    os(st, "建探测帧")?;
    let pb = Pb(pb);
    os(
        ffi::CVPixelBufferLockBaseAddress(pb.0, 0),
        "锁探测帧",
    )?;
    let base = ffi::CVPixelBufferGetBaseAddress(pb.0) as *mut u8;
    let stride = ffi::CVPixelBufferGetBytesPerRow(pb.0);
    for row in 0..size {
        std::ptr::write_bytes(base.add(row * stride), 0x80, size * 3 / 2);
    }
    ffi::CVPixelBufferUnlockBaseAddress(pb.0, 0);
    Ok(pb)
}

// ————————————————————————————————————————————————————————————
// 裸指针包装：跨回调通道搬运 CF 对象
// ————————————————————————————————————————————————————————————

/// 解码输出的像素缓冲。回调里 retain，Drop 时释放。
#[repr(transparent)]
struct Pb(ffi::CVPixelBufferRef);
unsafe impl Send for Pb {}

impl Pb {
    fn dims(&self) -> (usize, usize) {
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
struct CmSb(ffi::CMSampleBufferRef);
unsafe impl Send for CmSb {}

impl Drop for CmSb {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ffi::CFRelease(self.0) };
        }
    }
}

fn os(status: ffi::OSStatus, what: &str) -> AppResult<()> {
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
unsafe fn make_dict(entries: &[(ffi::CFTypeRef, ffi::CFTypeRef)]) -> ffi::CFMutableDictionaryRef {
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

unsafe fn make_int(v: i32) -> ffi::CFNumberRef {
    ffi::CFNumberCreate(
        ffi::kCFAllocatorDefault,
        ffi::kCFNumberSInt32Type,
        &v as *const i32 as *const c_void,
    )
}

/// IOSurface 背板声明：让解码输出/缩放目标都是 GPU 可共享的像素缓冲
unsafe fn iosurface_attrs() -> ffi::CFMutableDictionaryRef {
    let empty = make_dict(&[]);
    let attrs = make_dict(&[(
        ffi::kCVPixelBufferIOSurfacePropertiesKey as ffi::CFTypeRef,
        empty as ffi::CFTypeRef,
    )]);
    ffi::CFRelease(empty as ffi::CFTypeRef);
    attrs
}

// ————————————————————————————————————————————————————————————
// 解码
// ————————————————————————————————————————————————————————————

struct Decompressor {
    session: ffi::VTDecompressionSessionRef,
    format: ffi::CMVideoFormatDescriptionRef,
    rx: Receiver<Pb>,
    /// 回调 refcon 指进这个 Box，必须与 session 同生命周期
    _tx: Box<Sender<Pb>>,
}

impl Drop for Decompressor {
    fn drop(&mut self) {
        unsafe {
            ffi::VTDecompressionSessionInvalidate(self.session);
            ffi::CFRelease(self.session as ffi::CFTypeRef);
            ffi::CFRelease(self.format as ffi::CFTypeRef);
        }
    }
}

unsafe extern "C" fn decode_cb(
    refcon: *mut c_void,
    _source: *mut c_void,
    status: ffi::OSStatus,
    _info_flags: u32,
    image_buffer: ffi::CVPixelBufferRef,
    _pts: ffi::CMTime,
    _duration: ffi::CMTime,
) {
    if status != ffi::noErr || image_buffer.is_null() {
        return;
    }
    let tx = &*(refcon as *const Sender<Pb>);
    ffi::CFRetain(image_buffer as ffi::CFTypeRef);
    // 接收端已撤时 send 报错——会话正在销毁，忽略即可
    let _ = tx.send(Pb(image_buffer));
}

impl Decompressor {
    /// 用 `hvcC` 参数集（VPS/SPS/PPS 裸 NAL）建同步解码会话，输出 NV12。
    unsafe fn new(parameter_sets: &[Vec<u8>]) -> AppResult<Self> {
        let sizes: Vec<usize> = parameter_sets.iter().map(|n| n.len()).collect();
        let ptrs: Vec<*const u8> = parameter_sets.iter().map(|n| n.as_ptr()).collect();
        let mut format = std::ptr::null_mut();
        os(
            ffi::CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                ffi::kCFAllocatorDefault,
                parameter_sets.len(),
                ptrs.as_ptr(),
                sizes.as_ptr(),
                4,
                std::ptr::null(),
                &mut format,
            ),
            "建 HEVC 格式描述",
        )?;

        let pixel_format = make_int(ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
        let iosurface = iosurface_attrs();
        let attrs = make_dict(&[
            (
                ffi::kCVPixelBufferPixelFormatTypeKey as ffi::CFTypeRef,
                pixel_format as ffi::CFTypeRef,
            ),
            (
                ffi::kCVPixelBufferIOSurfacePropertiesKey as ffi::CFTypeRef,
                iosurface as ffi::CFTypeRef,
            ),
        ]);

        let (tx, rx) = channel::<Pb>();
        let boxed = Box::new(tx);
        let record = ffi::VTDecompressionOutputCallbackRecord {
            callback: Some(decode_cb),
            refcon: &*boxed as *const Sender<Pb> as *mut c_void,
        };
        let mut session: ffi::VTDecompressionSessionRef = std::ptr::null_mut();
        let status = ffi::VTDecompressionSessionCreate(
            ffi::kCFAllocatorDefault,
            format,
            std::ptr::null(),
            attrs,
            &record,
            &mut session,
        );
        ffi::CFRelease(attrs as ffi::CFTypeRef);
        ffi::CFRelease(iosurface as ffi::CFTypeRef);
        ffi::CFRelease(pixel_format as ffi::CFTypeRef);
        os(status, "建 HEVC 解码会话")?;

        Ok(Decompressor {
            session,
            format,
            rx,
            _tx: boxed,
        })
    }

    /// 解一个样本（MP4 原样字节：长度前缀 NAL，不需要转 Annex-B）。
    ///
    /// `pts_secs` 是该样本的显示时间：`DecodeFrame` 对**没有时间信息的样本**
    /// 直接回 kVTParameterErr（-12902），时间必须随样本给。
    unsafe fn decode(&self, sample: &[u8], pts_secs: f64, duration_secs: f64) -> AppResult<()> {
        let mut block = std::ptr::null_mut();
        os(
            ffi::CMBlockBufferCreateWithMemoryBlock(
                ffi::kCFAllocatorDefault,
                std::ptr::null_mut(),
                sample.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                sample.len(),
                0,
                &mut block,
            ),
            "建样本块",
        )?;
        os(
            ffi::CMBlockBufferReplaceDataBytes(
                sample.as_ptr() as *const c_void,
                block,
                0,
                sample.len(),
            ),
            "填充样本块",
        )?;
        let size = sample.len();
        const TS: i32 = 90_000;
        let timing = ffi::CMSampleTimingInfo {
            duration: ffi::CMTimeMake((duration_secs * f64::from(TS)).round() as i64, TS),
            presentationTimeStamp: ffi::CMTimeMake((pts_secs * f64::from(TS)).round() as i64, TS),
            decodeTimeStamp: ffi::kCMTimeInvalid,
        };
        let mut sbuf: ffi::CMSampleBufferRef = std::ptr::null_mut();
        os(
            ffi::CMSampleBufferCreateReady(
                ffi::kCFAllocatorDefault,
                block,
                self.format,
                1,
                1,
                &timing,
                1,
                &size,
                &mut sbuf,
            ),
            "建样本缓冲",
        )?;
        // 同步模式（不设异步标志）：输出回调在本次调用内触发
        let mut info_flags = 0u32;
        let status = ffi::VTDecompressionSessionDecodeFrame(
            self.session,
            sbuf,
            ffi::kVTDecodeFrame_Synchronous,
            std::ptr::null_mut(),
            &mut info_flags,
        );
        ffi::CFRelease(sbuf as ffi::CFTypeRef);
        os(status, "HEVC 解码")
    }

    /// 结束解码并收尾帧。
    unsafe fn finish(&self) -> AppResult<()> {
        os(
            ffi::VTDecompressionSessionFinishDelayedFrames(self.session),
            "收尾解码",
        )
    }

    /// 收走已就绪的输出帧（显示序）。
    fn drain(&self, out: &mut VecDeque<Pb>) {
        while let Ok(pb) = self.rx.try_recv() {
            out.push_back(pb);
        }
    }
}

// ————————————————————————————————————————————————————————————
// 编码
// ————————————————————————————————————————————————————————————

struct Compressor {
    session: ffi::VTCompressionSessionRef,
    rx: Receiver<CmSb>,
    /// 回调 refcon 指进这个 Box，必须与 session 同生命周期
    _tx: Box<Sender<CmSb>>,
    /// 首个输出样本的格式描述里藏着 SPS/PPS，取一次后复用
    parameter_sets: Option<Vec<u8>>,
}

impl Drop for Compressor {
    fn drop(&mut self) {
        unsafe {
            ffi::VTCompressionSessionInvalidate(self.session);
            ffi::CFRelease(self.session as ffi::CFTypeRef);
        }
    }
}

unsafe extern "C" fn encode_cb(
    refcon: *mut c_void,
    _source: *mut c_void,
    status: ffi::OSStatus,
    _info_flags: u32,
    sample_buffer: ffi::CMSampleBufferRef,
) {
    if status != ffi::noErr || sample_buffer.is_null() {
        return;
    }
    let tx = &*(refcon as *const Sender<CmSb>);
    ffi::CFRetain(sample_buffer as ffi::CFTypeRef);
    let _ = tx.send(CmSb(sample_buffer));
}

/// 会话的硬件策略：探测用 `Enable`（允许硬件、允许回落），真转码用
/// `Require`（探测已经确认硬件在位，回落反而是 bug）。
#[derive(Clone, Copy)]
enum HwPolicy {
    Require,
    Enable,
}

impl Compressor {
    /// 建硬编 H.264 会话。
    unsafe fn new(
        width: usize,
        height: usize,
        bitrate: i32,
        policy: HwPolicy,
    ) -> AppResult<Self> {
        let spec = make_dict(&[(
            match policy {
                HwPolicy::Require => {
                    ffi::kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder
                }
                HwPolicy::Enable => {
                    ffi::kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder
                }
            } as ffi::CFTypeRef,
            ffi::kCFBooleanTrue as ffi::CFTypeRef,
        )]);

        // 源缓冲属性：NV12 + IOSurface。解码输出是同格式同尺寸的 IOSurface
        // 背板缓冲，Apple Silicon 上编码器直接吃 GPU 表面，不过 CPU。
        let pixel_format = make_int(ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
        let w = make_int(width as i32);
        let h = make_int(height as i32);
        let iosurface = iosurface_attrs();
        let source_attrs = make_dict(&[
            (
                ffi::kCVPixelBufferPixelFormatTypeKey as ffi::CFTypeRef,
                pixel_format as ffi::CFTypeRef,
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
                iosurface as ffi::CFTypeRef,
            ),
        ]);

        let (tx, rx) = channel::<CmSb>();
        let boxed = Box::new(tx);
        let mut session: ffi::VTCompressionSessionRef = std::ptr::null_mut();
        let status = ffi::VTCompressionSessionCreate(
            ffi::kCFAllocatorDefault,
            width as i32,
            height as i32,
            ffi::kCMVideoCodecType_H264,
            spec,
            source_attrs,
            std::ptr::null(),
            Some(encode_cb),
            &*boxed as *const Sender<CmSb> as *mut c_void,
            &mut session,
        );
        ffi::CFRelease(source_attrs as ffi::CFTypeRef);
        ffi::CFRelease(iosurface as ffi::CFTypeRef);
        ffi::CFRelease(w as ffi::CFTypeRef);
        ffi::CFRelease(h as ffi::CFTypeRef);
        ffi::CFRelease(pixel_format as ffi::CFTypeRef);
        ffi::CFRelease(spec as ffi::CFTypeRef);
        os(status, "建 H.264 硬编会话")?;

        // 质量与码控。VT 没有 CRF：码率按分辨率×帧率标定（见 `bitrate_for`）。
        // MaxFrameDelayCount=0 关掉帧延迟（也就没有 B 帧重排）：输出顺序=输入
        // 顺序，pts 标注与 muxide 的「严格递增」要求都不受干扰——对齐软解
        // 路径 rusty_h264 的 lookahead=0 + num_ref_frames=1。
        // ProfileLevel 在个别第三方硬编上可能不吃：不致命，别让会话白建。
        let bitrate_num = make_int(bitrate);
        let gop = make_int(250);
        let no_delay = make_int(0);
        // 只有 AverageBitRate 是硬性要求（码控失效=质量目标失效）；其余
        // 尽力而为——实测部分编码器拒收 MaxFrameDelayCount=0 / 自定义 profile。
        // 被拒的后果：GOP 用编码器默认（约 2s，与 250 帧同档），帧延迟用
        // 编码器默认——时间戳标注读的是输出样本自带的 PTS（见 emit_outputs），
        // 不依赖「无重排」假设。
        let required = os(
            ffi::VTSessionSetProperty(
                session,
                ffi::kVTCompressionPropertyKey_AverageBitRate,
                bitrate_num as ffi::CFTypeRef,
            ),
            "设码率",
        );
        for (name, key, value) in [
            ("MaxKeyFrameInterval", ffi::kVTCompressionPropertyKey_MaxKeyFrameInterval, gop as ffi::CFTypeRef),
            ("MaxFrameDelayCount", ffi::kVTCompressionPropertyKey_MaxFrameDelayCount, no_delay as ffi::CFTypeRef),
        ] {
            let st = ffi::VTSessionSetProperty(session, key, value);
            if st != ffi::noErr {
                log::warn!("[Platform/vt] 编码器拒收 {name}（OSStatus {st}），用编码器默认值");
            }
        }
        let _ = ffi::VTSessionSetProperty(
            session,
            ffi::kVTCompressionPropertyKey_ProfileLevel,
            ffi::kVTProfileLevel_H264_High_AutoLevel as ffi::CFTypeRef,
        );
        let _ = ffi::VTSessionSetProperty(
            session,
            ffi::kVTCompressionPropertyKey_RealTime,
            ffi::kCFBooleanFalse as ffi::CFTypeRef,
        );
        ffi::CFRelease(bitrate_num as ffi::CFTypeRef);
        ffi::CFRelease(gop as ffi::CFTypeRef);
        ffi::CFRelease(no_delay as ffi::CFTypeRef);
        required?;

        Ok(Compressor {
            session,
            rx,
            _tx: boxed,
            parameter_sets: None,
        })
    }

    /// 编一帧（显示时间戳由调用方按源时间轴给）。
    unsafe fn encode(&mut self, frame: &Pb, pts_secs: f64, duration_secs: f64) -> AppResult<()> {
        const TS: i32 = 90_000;
        let pts = ffi::CMTimeMake((pts_secs * f64::from(TS)).round() as i64, TS);
        let dur = ffi::CMTimeMake((duration_secs * f64::from(TS)).round() as i64, TS);
        os(
            ffi::VTCompressionSessionEncodeFrame(
                self.session,
                frame.0,
                pts,
                dur,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            "编码帧",
        )
    }

    /// 收尾并冲出尾帧。
    unsafe fn finish(&mut self) -> AppResult<()> {
        os(
            ffi::VTCompressionSessionCompleteFrames(self.session, ffi::kCMTimeInvalid),
            "收尾编码",
        )
    }

    /// 编码器自报是否真走了硬件（`UsingHardwareAcceleratedVideoEncoder`）。
    unsafe fn using_hardware(&self) -> AppResult<bool> {
        let mut out: ffi::CFTypeRef = std::ptr::null();
        let st = ffi::VTSessionCopyProperty(
            self.session,
            ffi::kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
            ffi::kCFAllocatorDefault,
            &mut out,
        );
        if st != ffi::noErr || out.is_null() {
            return Ok(false);
        }
        let hw = ffi::CFBooleanGetValue(out as ffi::CFBooleanRef) != 0;
        ffi::CFRelease(out);
        Ok(hw)
    }

    /// 收走已就绪的编码输出（帧延迟已关：输出序=输入序）。
    fn take_outputs(&mut self) -> Vec<CmSb> {
        let mut out = Vec::new();
        while let Ok(sb) = self.rx.try_recv() {
            out.push(sb);
        }
        out
    }

    /// 把一个编码样本转成 muxide 要的 Annex-B；返回 `(Annex-B, 是否关键帧)`。
    ///
    /// 关键帧前插 SPS/PPS：拼接器重建 moov 时样本描述取自第 1 集，后续各集
    /// 的码流必须自带参数集才能在第 1 集的 avcC 之下解码。
    unsafe fn sample_to_unit(&mut self, sb: &CmSb) -> AppResult<(Vec<u8>, bool)> {
        let block = ffi::CMSampleBufferGetDataBuffer(sb.0);
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut at_offset = 0usize;
        let mut total = 0usize;
        os(
            ffi::CMBlockBufferGetDataPointer(block, 0, &mut at_offset, &mut total, &mut data),
            "读编码输出",
        )?;
        let raw = std::slice::from_raw_parts(data, total);
        // VT 输出的 H.264 是 4 字节长度前缀（AVCC）；Annex-B 转换与
        // media::hevc 是同一份实现
        let mut annexb = crate::media::hevc::to_annexb(raw, 4);

        // 关键帧（同步样本）前插 SPS/PPS；首个输出样本无论标没标都带上
        // （拼接器重建 moov 时样本描述取自第 1 集，后续各集码流必须自带
        // 参数集才能在第 1 集 avcC 之下解码）。第三个布尔是 is_keyframe，
        // 与 mux_h264 的契约一致——`sync` 就是「可独立解码」。
        let sync = is_sync_sample(sb.0);
        if sync || self.parameter_sets.is_none() {
            let desc = ffi::CMSampleBufferGetFormatDescription(sb.0);
            let params = parameter_sets_annexb(desc);
            if !params.is_empty() {
                if self.parameter_sets.is_none() {
                    self.parameter_sets = Some(params.clone());
                }
                annexb.splice(0..0, params);
            }
        }
        Ok((annexb, sync))
    }
}

/// `kCMSampleAttachmentKey_NotSync == false` 的样本才是可独立解码的同步样本。
/// 键不存在时按非同步处理（宁缺毋滥：标错关键帧会毁掉产物的 seek）。
unsafe fn is_sync_sample(sb: ffi::CMSampleBufferRef) -> bool {
    let array = ffi::CMSampleBufferGetSampleAttachmentsArray(sb, 0);
    if array.is_null() {
        return false;
    }
    let dict = ffi::CFArrayGetValueAtIndex(array, 0) as ffi::CFDictionaryRef;
    if dict.is_null() {
        return false;
    }
    let value =
        ffi::CFDictionaryGetValue(dict, ffi::kCMSampleAttachmentKey_NotSync as ffi::CFTypeRef);
    if value.is_null() {
        return false;
    }
    ffi::CFBooleanGetValue(value as ffi::CFBooleanRef) == 0
}

/// 格式描述里的参数集（SPS/PPS 裸 NAL）拼成 Annex-B。
unsafe fn parameter_sets_annexb(desc: ffi::CMVideoFormatDescriptionRef) -> Vec<u8> {
    let mut out = Vec::new();
    if desc.is_null() {
        return out;
    }
    // 总数先探一次（pointer/size 传 NULL 即可）
    let mut count = 0usize;
    if ffi::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        desc,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut count,
        std::ptr::null_mut(),
    ) != ffi::noErr
    {
        return out;
    }
    for i in 0..count {
        let mut ps: *const u8 = std::ptr::null();
        let mut size = 0usize;
        let st = ffi::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            desc,
            i,
            &mut ps,
            &mut size,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if st == ffi::noErr && !ps.is_null() && size > 0 {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(std::slice::from_raw_parts(ps, size));
        }
    }
    out
}

// ————————————————————————————————————————————————————————————
// 缩放
// ————————————————————————————————————————————————————————————

struct Transfer {
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
    unsafe fn new(width: usize, height: usize) -> AppResult<Self> {
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

    /// 缩到目标分辨率（NV12 → NV12，GPU 上完成）。
    unsafe fn scale(&self, src: &Pb) -> AppResult<Pb> {
        let mut dst: ffi::CVPixelBufferRef = std::ptr::null_mut();
        let attrs = iosurface_attrs();
        let st = ffi::CVPixelBufferCreate(
            ffi::kCFAllocatorDefault,
            self.width,
            self.height,
            ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            attrs,
            &mut dst,
        );
        ffi::CFRelease(attrs as ffi::CFTypeRef);
        os(st, "建缩放目标缓冲")?;
        let dst = Pb(dst);
        os(
            ffi::VTPixelTransferSessionTransferImage(self.session, src.0, dst.0),
            "缩放帧",
        )?;
        Ok(dst)
    }
}

// ————————————————————————————————————————————————————————————
// 编排
// ————————————————————————————————————————————————————————————

/// 码率标定：VT 没有 CRF，按「0.12 bit/像素/帧」给出与 libx264 `-crf 23`
/// 同档的码率（1080p25 ≈ 6.2 Mbps）。系数与 mf 侧同源——2026-10-07 在
/// Windows 侧用 NVENC MFT + libvmaf 双源复核后从 0.1 提到 0.12（高运动
/// 内容 CBR 偏低的收窄），VT 侧沿用同一档；偏差大时只调这个系数。
fn bitrate_for(width: usize, height: usize, fps: f64) -> i32 {
    let bps = width as f64 * height as f64 * fps.max(1.0) * 0.12;
    bps.clamp(800_000.0, 12_000_000.0) as i32
}

/// 下一帧的显示时间：按升序时间轴逐帧取，重复/回退按半帧顶开。
fn next_pts(sorted_pts: &[f64], cursor: &mut usize, last: &mut Option<f64>, frame_dur: f64) -> f64 {
    let mut p = sorted_pts
        .get(*cursor)
        .copied()
        .unwrap_or_else(|| last.unwrap_or(0.0) + frame_dur);
    *cursor += 1;
    if let Some(prev) = *last {
        if p <= prev {
            p = prev + frame_dur * 0.5;
        }
    }
    *last = Some(p);
    p
}

/// 一帧解码输出 →（缩放）→ 编码 → 收割产物。
#[allow(clippy::too_many_arguments)]
fn process_frame(
    pb: Pb,
    policy: HwPolicy,
    scale_to: Option<(usize, usize)>,
    fps: f64,
    frame_dur: f64,
    sorted_pts: &[f64],
    pts_cursor: &mut usize,
    last_pts: &mut Option<f64>,
    transfer: &mut Option<Transfer>,
    compressor: &mut Option<Compressor>,
    out_dims: &mut Option<(usize, usize)>,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    // 首帧才知道真实输出分辨率，编码会话延迟到这里建
    if compressor.is_none() {
        let (src_w, src_h) = pb.dims();
        let (out_w, out_h) = match scale_to {
            Some((w, h)) if (w, h) != (src_w, src_h) => {
                *transfer = Some(unsafe { Transfer::new(w, h)? });
                (w, h)
            }
            Some((w, h)) => (w, h),
            None => (src_w, src_h),
        };
        *compressor = Some(unsafe {
            Compressor::new(out_w, out_h, bitrate_for(out_w, out_h, fps), policy)?
        });
        *out_dims = Some((out_w, out_h));
    }
    let frame = match transfer.as_ref() {
        Some(t) => unsafe { t.scale(&pb) }?,
        None => pb,
    };
    let compressor = compressor.as_mut().expect("上一行刚建好");
    let pts = next_pts(sorted_pts, pts_cursor, last_pts, frame_dur);
    unsafe { compressor.encode(&frame, pts, frame_dur)? };
    emit_outputs(compressor, last_pts, frame_dur, units, on_progress)
}

/// 收割编码器已就绪的输出。
///
/// 显示时间读**输出样本自带的 PTS**（编码器保留编码时传入的时间戳），
/// 不依赖「输出序=输入序」——个别编码器拒收 MaxFrameDelayCount=0 时可能
/// 重排，arrival 序标时间会整体错位。递增兜底仍在：畸形时间按半帧顶开。
#[allow(clippy::too_many_arguments)]
fn emit_outputs(
    compressor: &mut Compressor,
    last_pts: &mut Option<f64>,
    frame_dur: f64,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    for sb in compressor.take_outputs() {
        let (annexb, keyframe) = unsafe { compressor.sample_to_unit(&sb) }?;
        let raw = output_pts_secs(&sb);
        let pts = match *last_pts {
            Some(prev) if raw <= prev => prev + frame_dur * 0.5,
            _ => raw,
        };
        *last_pts = Some(pts);
        units.push((pts, annexb, keyframe));
        if let Some(cb) = on_progress {
            cb(pts);
        }
    }
    Ok(())
}

/// 输出样本的显示时间（秒）。timescale 非法时按 0 处理，交给递增兜底。
fn output_pts_secs(sb: &CmSb) -> f64 {
    let t = unsafe { ffi::CMSampleBufferGetOutputPresentationTimeStamp(sb.0) };
    if t.timescale <= 0 {
        0.0
    } else {
        t.value as f64 / f64::from(t.timescale)
    }
}

fn run(
    req: &PlatformRequest<'_>,
    demuxed: &crate::media::demux::Demuxed,
    video: &TrackInfo,
    policy: HwPolicy,
) -> AppResult<()> {
    let started = Instant::now();

    // 1) 参数集与源时间轴。hvcC 里可能混着非参数集 NAL（实测 libx265 会把
    //    两 KB 的 SEI 塞进 hvcC），格式描述只收 VPS/SPS/PPS，其余剔除。
    let (raw_sets, _) = crate::media::hevc::read_parameter_set_nalus(req.input, video)?;
    let parameter_sets: Vec<Vec<u8>> = raw_sets
        .into_iter()
        .filter(|n| !n.is_empty() && matches!((n[0] >> 1) & 0x3f, 32..=34))
        .collect();
    let fps = video.average_framerate().unwrap_or(25.0);
    let frame_dur = 1.0 / fps;
    // 解码输出按显示序出帧：显示时间按升序排列后逐帧对位
    let mut source_pts = video.sample_pts();
    let mut sorted_pts = source_pts.clone();
    if sorted_pts.is_empty() {
        let synthetic: Vec<f64> = (0..video.samples.len()).map(|i| i as f64 / fps).collect();
        source_pts = synthetic.clone();
        sorted_pts = synthetic;
    } else {
        sorted_pts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));

    // 2) 解码 → （缩放）→ 编码 流水
    let decoder = unsafe { Decompressor::new(&parameter_sets)? };
    let mut pending: VecDeque<Pb> = VecDeque::new();
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut transfer: Option<Transfer> = None;
    let mut compressor: Option<Compressor> = None;
    let mut out_dims: Option<(usize, usize)> = None;
    let mut pts_cursor = 0usize;
    let mut last_pts: Option<f64> = None;

    for (i, &(offset, size)) in video.samples.iter().enumerate() {
        let raw = crate::media::transcode::read_range(req.input, offset, size)?;
        if raw.is_empty() {
            continue;
        }
        let pts = source_pts.get(i).copied().unwrap_or(0.0);
        unsafe { decoder.decode(&raw, pts, frame_dur)? };

        decoder.drain(&mut pending);
        while let Some(pb) = pending.pop_front() {
            process_frame(
                pb,
                policy,
                scale_to,
                fps,
                frame_dur,
                &sorted_pts,
                &mut pts_cursor,
                &mut last_pts,
                &mut transfer,
                &mut compressor,
                &mut out_dims,
                &mut units,
                req.on_progress,
            )?;
        }
    }
    unsafe { decoder.finish()? };
    decoder.drain(&mut pending);
    while let Some(pb) = pending.pop_front() {
        process_frame(
            pb,
            policy,
            scale_to,
            fps,
            frame_dur,
            &sorted_pts,
            &mut pts_cursor,
            &mut last_pts,
            &mut transfer,
            &mut compressor,
            &mut out_dims,
            &mut units,
            req.on_progress,
        )?;
    }

    // 3) 冲出编码器尾帧
    let mut compressor = match compressor {
        Some(c) => c,
        None => return Err(AppError::Media("HEVC 解码没有产出任何帧".into())),
    };
    unsafe { compressor.finish()? };
    emit_outputs(
        &mut compressor,
        &mut last_pts,
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
        req.input,
        demuxed,
        req.output,
        &units,
        out_w,
        out_h,
        fps as f32,
    )?;

    log::info!(
        "[Platform/vt] HEVC→H.264 完成：{} 帧（{out_w}x{out_h}），耗时 {:?}",
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
        // 1080p25 → 6.22 Mbps：与 libx264 crf23 的产物同档（0.12 系数）
        let b = bitrate_for(1920, 1080, 25.0);
        assert!((5_900_000..=6_600_000).contains(&b), "实际 {b}");
        // 夹在合理区间
        assert_eq!(bitrate_for(1920, 1080, 240.0), 12_000_000);
        assert_eq!(bitrate_for(320, 180, 10.0), 800_000);
    }

    #[test]
    fn probe_never_panics_without_a_gpu() {
        // 无 GPU/被禁用的机器上安静地返回
        let _ = h264_hw_encoder_available();
    }
}
