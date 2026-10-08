//! macOS VideoToolbox 编码后端（硬编优先，无硬编回落 Apple 内置软编会话）。
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
//! 探测只回答两个问题：会话能不能建（生产闸门——软编会话也算数）、
//! 是不是硬件（能力徽标与并行度）。纪律与 ffmpeg 侧一致：「能建会话」
//! 不算硬件，编码器自己说「我走了硬件」才算；256×256 的下限与 ffmpeg 侧
//! nvenc 的教训一致（更小的探测帧会被最小分辨率拒掉，把可用的硬编误判成
//! 不可用）。

mod ffi;

use std::os::raw::c_void;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use parking_lot::RwLock;

use crate::domain::mp4::sample_table::TrackInfo;
use crate::error::{AppError, AppResult};

use super::PlatformRequest;

/// 探测结果缓存：「会话能建吗」与「会话是不是硬件」分开回答——前者决定
/// 生产链路是否走 VT（软编会话也算数），后者只喂能力徽标与并行度。
#[derive(Clone, Copy)]
struct ProbeAnswer {
    /// VT 编码会话可建（硬件或 Apple 内置软编）
    available: bool,
    /// 实编一帧后编码器自报走了硬件
    hardware: bool,
}

static PROBE: RwLock<Option<ProbeAnswer>> = RwLock::new(None);

/// 平台编码层是否可用（VT 会话可建即真，含 Apple 软编会话）。
pub fn encoder_available() -> bool {
    probe_once().available
}

/// 平台**硬编**是否可用。不再是生产闸门，只喂能力徽标与并行度策略。
pub fn h264_hw_encoder_available() -> bool {
    probe_once().hardware
}

fn probe_once() -> ProbeAnswer {
    let mut slot = PROBE.write();
    if slot.is_none() {
        *slot = Some(match unsafe { probe_reports_hardware() } {
            Ok(hardware) => ProbeAnswer {
                available: true,
                hardware,
            },
            Err(e) => {
                log::warn!("[Platform/vt] 平台编码探测失败，按不可用处理: {e}");
                ProbeAnswer {
                    available: false,
                    hardware: false,
                }
            }
        });
    }
    slot.unwrap_or(ProbeAnswer {
        available: false,
        hardware: false,
    })
}

/// 丢弃探测缓存。
pub fn clear_probe_cache() {
    *PROBE.write() = None;
}

/// 平台编码层执行 HEVC→H.264 转码；HEVC 以外的源返回 `None`
/// （留给 ffmpeg/软解——H.264 源在 ffmpeg 侧是直转，没必要绕 GPU）。
///
/// 闸门是「VT 会话能建」而不是「必须硬编」：会话有硬编走硬编，没有就落
/// Apple 内置软编会话——后者仍然显著快于 ffmpeg 与纯 Rust 软解（本机 i7
/// 实测 44.9s 集：VT 链 ≈5.9s，libx264 18.2s、纯 Rust 54s），且同样不依赖
/// 用户装任何东西。会话真建不出来（罕见）由管线落回下一条路。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let demuxed = crate::media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !crate::media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run(req, &demuxed, &video.info))
}

/// 探测纪律与 ffmpeg 侧同一套：「能建会话」不算数，编码器自己说「我走了
/// 硬件」才算。`RequireHardware` 在 Intel Mac 上常见 -12903 误报（明明有
/// QuickSync 却枚举不到），所以用 `Enable`（允许硬件、不强制）建会话，
/// **实编一帧**后读 `UsingHardwareAcceleratedVideoEncoder`。
///
/// 探测帧必须用**真实分辨率**（1080p）：VT 按分辨率挑编码器实例，小帧
/// 会落到软编实例上、真实会话却是硬件——本机实证过（256×256 报 hw=false，
/// 生产 1080p 会话 CPU 8% 跑 2× 实时/路，明显是硬件）。分辨率下限的教训
/// 与 ffmpeg 侧 nvenc（64×64 误判不可用）同源：探测形状要贴近真实使用。
unsafe fn probe_reports_hardware() -> AppResult<bool> {
    unsafe {
        let mut c = Compressor::new(1920, 1080, 6_000_000)?;
        let frame = make_test_frame(1920)?;
        c.encode(&frame, 0.0, 1.0 / 30.0)?;
        c.finish()?;
        let outputs = c.take_outputs();
        drop(outputs);
        c.using_hardware()
    }
}

/// 探测用的灰帧：NV12 平面铺 0x80（中性灰），避免未初始化内存进编码器。
/// `width` 作宽度、1080 作高度（探测会话与生产会话同分辨率量级）。
unsafe fn make_test_frame(width: usize) -> AppResult<Pb> {
    unsafe {
        let height = 1080;
        let mut pb: ffi::CVPixelBufferRef = std::ptr::null_mut();
        let attrs = iosurface_attrs();
        let st = ffi::CVPixelBufferCreate(
            ffi::kCFAllocatorDefault,
            width,
            height,
            ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            attrs,
            &mut pb,
        );
        ffi::CFRelease(attrs as ffi::CFTypeRef);
        os(st, "建探测帧")?;
        let pb = Pb(pb);
        os(ffi::CVPixelBufferLockBaseAddress(pb.0, 0), "锁探测帧")?;
        // 逐平面逐行铺 0x80。IOSurface 的整块分配不等于「stride×高×1.5」的
        // 纸面加和（平面各自对齐）——按整块 memset 实测越界段错误。
        for plane in 0..ffi::CVPixelBufferGetPlaneCount(pb.0) {
            let base = ffi::CVPixelBufferGetBaseAddressOfPlane(pb.0, plane) as *mut u8;
            let stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pb.0, plane);
            let rows = ffi::CVPixelBufferGetHeightOfPlane(pb.0, plane);
            for row in 0..rows {
                std::ptr::write_bytes(base.add(row * stride), 0x80, stride);
            }
        }
        ffi::CVPixelBufferUnlockBaseAddress(pb.0, 0);
        Ok(pb)
    }
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

unsafe fn make_int(v: i32) -> ffi::CFNumberRef {
    unsafe {
        ffi::CFNumberCreate(
            ffi::kCFAllocatorDefault,
            ffi::kCFNumberSInt32Type,
            &v as *const i32 as *const c_void,
        )
    }
}

/// IOSurface 背板声明：让解码输出/缩放目标都是 GPU 可共享的像素缓冲
unsafe fn iosurface_attrs() -> ffi::CFMutableDictionaryRef {
    unsafe {
        let empty = make_dict(&[]);
        let attrs = make_dict(&[(
            ffi::kCVPixelBufferIOSurfacePropertiesKey as ffi::CFTypeRef,
            empty as ffi::CFTypeRef,
        )]);
        ffi::CFRelease(empty as ffi::CFTypeRef);
        attrs
    }
}

// ————————————————————————————————————————————————————————————
// 解码
// ————————————————————————————————————————————————————————————

struct Decompressor {
    session: ffi::VTDecompressionSessionRef,
    format: ffi::CMVideoFormatDescriptionRef,
    rx: Receiver<(Pb, f64)>,
    /// 回调 refcon 指进这个 Box，必须与 session 同生命周期
    _tx: Box<Sender<(Pb, f64)>>,
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
    pts: ffi::CMTime,
    _duration: ffi::CMTime,
) {
    unsafe {
        if status != ffi::noErr || image_buffer.is_null() {
            return;
        }
        let tx = &*(refcon as *const Sender<(Pb, f64)>);
        ffi::CFRetain(image_buffer as ffi::CFTypeRef);
        // 帧连同它自己的显示时间一起上交——同步解码不做 B 帧重排，吐出的是
        // 解码序，下游必须按这个 PTS 配对显示槽，绝不按吐出顺序排位
        let _ = tx.send((Pb(image_buffer), cm_time_secs(pts)));
    }
}

impl Decompressor {
    /// 用 `hvcC` 参数集（VPS/SPS/PPS 裸 NAL）建同步解码会话，输出 NV12。
    unsafe fn new(parameter_sets: &[Vec<u8>]) -> AppResult<Self> {
        unsafe {
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

            let pixel_format =
                make_int(ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
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

            let (tx, rx) = channel::<(Pb, f64)>();
            let boxed = Box::new(tx);
            let record = ffi::VTDecompressionOutputCallbackRecord {
                callback: Some(decode_cb),
                refcon: &*boxed as *const Sender<(Pb, f64)> as *mut c_void,
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
    }

    /// 解一个样本（MP4 原样字节：长度前缀 NAL，不需要转 Annex-B）。
    ///
    /// `pts_secs` 是该样本的显示时间：`DecodeFrame` 对**没有时间信息的样本**
    /// 直接回 kVTParameterErr（-12902），时间必须随样本给。
    unsafe fn decode(&self, sample: &[u8], pts_secs: f64, duration_secs: f64) -> AppResult<()> {
        unsafe {
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
                presentationTimeStamp: ffi::CMTimeMake(
                    (pts_secs * f64::from(TS)).round() as i64,
                    TS,
                ),
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
    }

    /// 结束解码并收尾帧。
    unsafe fn finish(&self) -> AppResult<()> {
        unsafe {
            os(
                ffi::VTDecompressionSessionFinishDelayedFrames(self.session),
                "收尾解码",
            )
        }
    }

    /// 收走已就绪的输出帧（解码序——同步解码无重排，顺序不可信）。
    fn drain(&self, out: &mut Vec<(Pb, f64)>) {
        while let Ok(item) = self.rx.try_recv() {
            out.push(item);
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
    unsafe {
        if status != ffi::noErr || sample_buffer.is_null() {
            return;
        }
        let tx = &*(refcon as *const Sender<CmSb>);
        ffi::CFRetain(sample_buffer as ffi::CFTypeRef);
        let _ = tx.send(CmSb(sample_buffer));
    }
}

impl Compressor {
    /// 建 H.264 编码会话：`Enable` 策略——有硬编用硬编，没有就落 Apple
    /// 内置软编会话。曾经还有 `Require`（探测确认硬编后才放行生产），
    /// 软编会话转正后两态合一，探测的 hw/sw 结论只喂徽标与并行度。
    unsafe fn new(width: usize, height: usize, bitrate: i32) -> AppResult<Self> {
        unsafe {
            let spec = make_dict(&[(
                ffi::kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder
                    as ffi::CFTypeRef,
                ffi::kCFBooleanTrue as ffi::CFTypeRef,
            )]);

            // 源缓冲属性：NV12 + IOSurface。解码输出是同格式同尺寸的 IOSurface
            // 背板缓冲，Apple Silicon 上编码器直接吃 GPU 表面，不过 CPU。
            let pixel_format =
                make_int(ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
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
            let no_reorder = make_int(0);
            // 只有 AverageBitRate 是硬性要求（码控失效=质量目标失效）；其余
            // 尽力而为——实测部分编码器拒收 MaxFrameDelayCount=0 / 自定义 profile。
            // **AllowFrameReordering 必须关掉**：B 帧重排一旦被编码器打开（硬编
            // 会话会拒收 MaxFrameDelayCount=0，本机实证），输出落进解码序 +
            // 非单调 PTS，递增兜底只能把乱序帧顶到错误的时间上——产物帧序乱掉
            // （真实剧集 B 帧流首跑即现）。下游 mux 的契约就是单调显示序。
            let required = os(
                ffi::VTSessionSetProperty(
                    session,
                    ffi::kVTCompressionPropertyKey_AverageBitRate,
                    bitrate_num as ffi::CFTypeRef,
                ),
                "设码率",
            );
            for (name, key, value) in [
                (
                    "AllowFrameReordering",
                    ffi::kVTCompressionPropertyKey_AllowFrameReordering,
                    no_reorder as ffi::CFTypeRef,
                ),
                (
                    "MaxKeyFrameInterval",
                    ffi::kVTCompressionPropertyKey_MaxKeyFrameInterval,
                    gop as ffi::CFTypeRef,
                ),
                (
                    "MaxFrameDelayCount",
                    ffi::kVTCompressionPropertyKey_MaxFrameDelayCount,
                    no_delay as ffi::CFTypeRef,
                ),
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
    }

    /// 编一帧（显示时间戳由调用方按源时间轴给）。
    unsafe fn encode(&mut self, frame: &Pb, pts_secs: f64, duration_secs: f64) -> AppResult<()> {
        unsafe {
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
    }

    /// 收尾并冲出尾帧。
    unsafe fn finish(&mut self) -> AppResult<()> {
        unsafe {
            os(
                ffi::VTCompressionSessionCompleteFrames(self.session, ffi::kCMTimeInvalid),
                "收尾编码",
            )
        }
    }

    /// 编码器自报是否真走了硬件（`UsingHardwareAcceleratedVideoEncoder`）。
    unsafe fn using_hardware(&self) -> AppResult<bool> {
        unsafe {
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
        unsafe {
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
}

/// VT 编码输出的同步样本判定：`kCMSampleAttachmentKey_NotSync` 缺省或为 false
/// 即同步样本。压缩输出的约定是**非同步帧必带 NotSync=true，同步帧省略该键**
/// （Apple 示例代码一律按 `!contains(key)` 判）——首帧 IDR 不带这个键，
/// 按「缺省即非同步」判会把首帧标成非关键帧，muxide 直接拒收。
unsafe fn is_sync_sample(sb: ffi::CMSampleBufferRef) -> bool {
    unsafe {
        let array = ffi::CMSampleBufferGetSampleAttachmentsArray(sb, 0);
        if array.is_null() {
            return true;
        }
        let dict = ffi::CFArrayGetValueAtIndex(array, 0) as ffi::CFDictionaryRef;
        if dict.is_null() {
            return true;
        }
        let value =
            ffi::CFDictionaryGetValue(dict, ffi::kCMSampleAttachmentKey_NotSync as ffi::CFTypeRef);
        if value.is_null() {
            return true;
        }
        ffi::CFBooleanGetValue(value as ffi::CFBooleanRef) == 0
    }
}

/// 格式描述里的参数集（SPS/PPS 裸 NAL）拼成 Annex-B。
unsafe fn parameter_sets_annexb(desc: ffi::CMVideoFormatDescriptionRef) -> Vec<u8> {
    unsafe {
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

    /// 缩到目标分辨率（NV12 → NV12，GPU 上完成）。
    unsafe fn scale(&self, src: &Pb) -> AppResult<Pb> {
        unsafe {
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

/// 一帧解码输出 →（缩放）→ 编码 → 收割产物。`pts` 是该帧与源槽位配对
/// 好的显示时间（见 run() 流水注释）。
#[expect(
    clippy::too_many_arguments,
    reason = "参数表与平台 C API（虚表/会话属性）一一对应，硬拆参数结构反而失真"
)]
fn process_frame(
    pb: Pb,
    pts: f64,
    scale_to: Option<(usize, usize)>,
    fps: f64,
    frame_dur: f64,
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
        let c = unsafe { Compressor::new(out_w, out_h, bitrate_for(out_w, out_h, fps))? };
        // 编码器自报硬编还是软编——只进日志，产物两边都是能播的 H.264
        let hw = unsafe { c.using_hardware() }.unwrap_or(false);
        log::info!(
            "[Platform/vt] 编码会话已建（{out_w}x{out_h}）：{}",
            if hw { "硬件" } else { "Apple 软编" }
        );
        *compressor = Some(c);
        *out_dims = Some((out_w, out_h));
    }
    let frame = match transfer.as_ref() {
        Some(t) => unsafe { t.scale(&pb) }?,
        None => pb,
    };
    let compressor = compressor.as_mut().expect("上一行刚建好");
    unsafe { compressor.encode(&frame, pts, frame_dur)? };
    emit_outputs(compressor, units, on_progress)
}

/// 收割编码器已就绪的输出。
///
/// 显示时间读**输出样本自带的 PTS**（编码器保留编码时传入的时间戳），
/// 不依赖「输出序=输入序」——个别编码器拒收 MaxFrameDelayCount=0 时可能
/// 重排，arrival 序标时间会整体错位。递增兜底仍在：畸形时间按半帧顶开。
fn emit_outputs(
    compressor: &mut Compressor,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    for sb in compressor.take_outputs() {
        let (annexb, keyframe) = unsafe { compressor.sample_to_unit(&sb) }?;
        // PTS **原样透传**。解码侧按源时间轴精确配对后，发射序=解码序，
        // 显示时间天然非单调——任何「回退顶开」都会把真实显示顺序打乱
        // （真实剧集 B 帧流首跑实证：标题帧被顶进转场中间）。非单调交给
        // muxide：按时间戳交织、写 ctts，这正是 MP4 B 帧的标准形态。
        let pts = output_pts_secs(&sb);
        units.push((pts, annexb, keyframe));
        if let Some(cb) = on_progress {
            cb(pts);
        }
    }
    Ok(())
}

/// CMTime → 秒。timescale 非法时按 0 处理，交给递增兜底。
fn cm_time_secs(t: ffi::CMTime) -> f64 {
    if t.timescale <= 0 {
        0.0
    } else {
        t.value as f64 / f64::from(t.timescale)
    }
}

/// 输出样本的显示时间（秒）。
fn output_pts_secs(sb: &CmSb) -> f64 {
    cm_time_secs(unsafe { ffi::CMSampleBufferGetOutputPresentationTimeStamp(sb.0) })
}

/// 显示时间 → 90000 刻度整数键：与 decode() 喂给 VT 的取整同一刻度，
/// 解码回调带回的值可与源槽位精确相等，用作帧—时间槽的配对键。
fn ptkey(secs: f64) -> i64 {
    (secs * 90_000.0).round() as i64
}

fn run(
    req: &PlatformRequest<'_>,
    demuxed: &crate::media::demux::Demuxed,
    video: &TrackInfo,
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
    // 每个样本自己的显示时间（stts 累计 + ctts 合成偏移）；空表退化为按
    // 平均帧率均摊。帧与时间槽的配对全靠它，见下面的流水注释。
    let mut source_pts = video.sample_pts();
    if source_pts.is_empty() {
        source_pts = (0..video.samples.len()).map(|i| i as f64 / fps).collect();
    }
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));

    // 2) 解码 → （缩放）→ 编码 流水。两个「顺序不可信」，各修各的：
    //
    // - **解码吐出序不可信**：同步解码（回调在 decode() 内触发）没有
    //   B 帧重排，吐出的是解码序；解码回调把喂入的显示时间原样带回，
    //   帧↔显示时间按 PTS 认领，绝不按吐出顺序排位。
    // - **编码器输入必须是显示序**：编码器契约按显示序收帧、自行构造
    //   B 帧 GOP 并写 POC。把解码序直接喂进去，POC 会把解码序固化成
    //   显示序——产物按解码序播放（真实剧集实证：标题帧闪现在转场前，
    //   showinfo 呈现序 0, 0.533, 0.067…）。配对好的帧按容器时间槽顺序
    //   喂编码器；槽内帧未到就继续解码下一个样本，内存以重排窗口为界。
    let decoder = unsafe { Decompressor::new(&parameter_sets)? };
    let mut slot_times = source_pts.clone();
    slot_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut ready: Vec<(Pb, f64)> = Vec::new();
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut transfer: Option<Transfer> = None;
    let mut compressor: Option<Compressor> = None;
    let mut out_dims: Option<(usize, usize)> = None;
    let mut slot = 0usize;

    let feed_ready_slots = |ready: &mut Vec<(Pb, f64)>,
                            slot: &mut usize,
                            transfer: &mut Option<Transfer>,
                            compressor: &mut Option<Compressor>,
                            out_dims: &mut Option<(usize, usize)>,
                            units: &mut Vec<(f64, Vec<u8>, bool)>|
     -> AppResult<()> {
        while *slot < slot_times.len() {
            let key = ptkey(slot_times[*slot]);
            let Some(pos) = ready.iter().position(|(_, k)| ptkey(*k) == key) else {
                break;
            };
            let (pb, pts) = ready.swap_remove(pos);
            process_frame(
                pb,
                pts,
                scale_to,
                fps,
                frame_dur,
                transfer,
                compressor,
                out_dims,
                units,
                req.on_progress,
            )?;
            *slot += 1;
        }
        Ok(())
    };

    for (i, &(offset, size)) in video.samples.iter().enumerate() {
        let raw = crate::media::transcode::read_range(req.input, offset, size)?;
        if raw.is_empty() {
            continue;
        }
        let pts = source_pts.get(i).copied().unwrap_or(0.0);
        unsafe { decoder.decode(&raw, pts, frame_dur)? };
        decoder.drain(&mut ready);
        feed_ready_slots(
            &mut ready,
            &mut slot,
            &mut transfer,
            &mut compressor,
            &mut out_dims,
            &mut units,
        )?;
    }
    unsafe { decoder.finish()? };
    // 收尾：同步解码逐帧结清，此处正常应为空；万一有遗留，按时间槽补齐
    decoder.drain(&mut ready);
    feed_ready_slots(
        &mut ready,
        &mut slot,
        &mut transfer,
        &mut compressor,
        &mut out_dims,
        &mut units,
    )?;
    // 时间槽全过完后仍剩余的帧（源时间轴之外的重复/畸形）——按 PTS 排序追加
    ready.sort_by_key(|(_, pts)| ptkey(*pts));
    for (pb, pts) in ready {
        process_frame(
            pb,
            pts,
            scale_to,
            fps,
            frame_dur,
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
    emit_outputs(&mut compressor, &mut units, req.on_progress)?;

    if units.is_empty() {
        return Err(AppError::Media("平台硬编没有产出任何帧".into()));
    }

    // 4) 音轨直通 + 封装（与软解路径同一套代码，faststart）
    let (out_w, out_h) = out_dims.unwrap_or((video.width as usize, video.height as usize));
    crate::media::transcode::mux_with_audio(
        req.input, demuxed, req.output, &units, out_w, out_h, fps as f32,
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
