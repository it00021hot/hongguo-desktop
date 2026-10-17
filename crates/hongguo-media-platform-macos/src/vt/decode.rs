//! HEVC / H.264 解码：同步解码会话（[`Decompressor`]，VTDecompressionSession）
//! 与播放末端的逐帧解码器（[`FrameDecoder`] / [`DecodedFrame`]）。
//!
//! 解码输出回调 [`decode_cb`] 的 refcon 裸指针指向会话内部的 `Box<Sender>`
//! 队列——回调与所属类型必须同文件。

use std::os::raw::c_void;
use std::sync::mpsc::{Receiver, Sender, channel};

use core_video::pixel_buffer::{CVPixelBuffer, CVPixelBufferRef};

use hongguo_common::AppResult;
use hongguo_media_platform_api::video::VideoCodec;

use super::ffi;
use super::pipeline::cm_time_secs;
use super::pixel::{Pb, Transfer, iosurface_attrs, make_dict, make_int, os};

// ————————————————————————————————————————————————————————————
// 解码
// ————————————————————————————————————————————————————————————

/// 查出刚建的解码会话是不是硬件解码，记一行日志。
///
/// 排障必需：软解在 1080p HEVC 上是 46.8ms/帧（实时线 33ms），表现是
/// 「画面越播越落后音频」，而日志里光看「首帧正常」根本看不出来。
unsafe fn log_hardware(session: ffi::VTDecompressionSessionRef, label: &str) {
    unsafe {
        let mut value: ffi::CFTypeRef = std::ptr::null();
        let status = ffi::VTSessionCopyProperty(
            session as ffi::VTSessionRef,
            ffi::kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder,
            ffi::kCFAllocatorDefault,
            &mut value,
        );
        if status == ffi::noErr && !value.is_null() {
            let hw = ffi::CFBooleanGetValue(value as ffi::CFBooleanRef) != 0;
            ffi::CFRelease(value);
            log::info!(
                "[Platform/vt] {label} 解码会话：{}",
                if hw {
                    "硬件解码"
                } else {
                    "软解（VT 未启用硬件）"
                }
            );
        } else {
            log::debug!("[Platform/vt] 解码会话硬件标志不可读（status {status}）");
        }
    }
}

pub(super) struct Decompressor {
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
    /// 用参数集（HEVC 的 VPS/SPS/PPS，或 H.264 的 SPS/PPS，均为裸 NAL）建
    /// 同步解码会话，输出 NV12。
    ///
    /// `full_range` 选输出像素区间：`false` = VideoRange（'420v'，转码链
    /// 既定格式）；`true` = FullRange（'420f'，GPU 合成器直采样要求的形态，
    /// 见 [`FrameDecoder`]）。
    pub(super) unsafe fn new(
        codec: VideoCodec,
        parameter_sets: &[Vec<u8>],
        full_range: bool,
    ) -> AppResult<Self> {
        unsafe {
            let label = match codec {
                VideoCodec::Hevc => "HEVC",
                VideoCodec::H264 => "H.264",
            };
            let sizes: Vec<usize> = parameter_sets.iter().map(|n| n.len()).collect();
            let ptrs: Vec<*const u8> = parameter_sets.iter().map(|n| n.as_ptr()).collect();
            let mut format = std::ptr::null_mut();
            // 两种编码的参数集顺序各有讲究：HEVC 要 VPS/SPS/PPS，H.264 要
            // SPS/PPS（引擎侧提取函数按这个次序给出）。样本长度前缀恒 4
            // （hvcC/avcC 声明的 length_size，引擎侧提取时已归一校验）。
            match codec {
                VideoCodec::Hevc => os(
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
                )?,
                VideoCodec::H264 => os(
                    ffi::CMVideoFormatDescriptionCreateFromH264ParameterSets(
                        ffi::kCFAllocatorDefault,
                        parameter_sets.len(),
                        ptrs.as_ptr(),
                        sizes.as_ptr(),
                        4,
                        &mut format,
                    ),
                    "建 H.264 格式描述",
                )?,
            };

            let pixel_format = make_int(if full_range {
                ffi::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            } else {
                ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            } as i32);
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
            // 显式点名硬件解码：不传规格时 VT 可能落软解，1080p HEVC 软解
            // 46.8ms/帧（>33ms 的实时线），画面必然越播越落后。
            // 只「建议」不「要求」：个别码流硬解不收时还得让 VT 回软解，
            // 能播总比报错强（建的会话是什么，下面查出来记日志）。
            let spec = make_dict(&[(
                ffi::kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder
                    as ffi::CFTypeRef,
                ffi::kCFBooleanTrue as ffi::CFTypeRef,
            )]);
            let mut session: ffi::VTDecompressionSessionRef = std::ptr::null_mut();
            let status = ffi::VTDecompressionSessionCreate(
                ffi::kCFAllocatorDefault,
                format,
                spec,
                attrs,
                &record,
                &mut session,
            );
            ffi::CFRelease(spec as ffi::CFTypeRef);
            ffi::CFRelease(attrs as ffi::CFTypeRef);
            ffi::CFRelease(iosurface as ffi::CFTypeRef);
            ffi::CFRelease(pixel_format as ffi::CFTypeRef);
            os(status, "建解码会话")?;
            log_hardware(session, label);

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
    pub(super) unsafe fn decode(
        &self,
        sample: &[u8],
        pts_secs: f64,
        duration_secs: f64,
    ) -> AppResult<()> {
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
            // 异步模式：`kVTDecodeFrame_EnableAsynchronousDecompression` 要求
            // VT 把解码排队到内部队列，DecodeFrame 立即返回、结果经回调异步
            // 到达。同步模式（不设标志）会让本调用阻塞到帧出来——实测 1080p
            // HEVC 每帧 46.5ms，硬件再快也排不成流水线，30fps 的实时线是
            // 33ms/帧，同步模式必然越播越落后。异步让硬件并行吃满。
            let mut info_flags = 0u32;
            let status = ffi::VTDecompressionSessionDecodeFrame(
                self.session,
                sbuf,
                ffi::kVTDecodeFrame_EnableAsynchronousDecompression,
                std::ptr::null_mut(),
                &mut info_flags,
            );
            ffi::CFRelease(sbuf as ffi::CFTypeRef);
            os(status, "HEVC 解码")
        }
    }

    /// 结束解码并收尾帧。
    pub(super) unsafe fn finish(&self) -> AppResult<()> {
        unsafe {
            os(
                ffi::VTDecompressionSessionFinishDelayedFrames(self.session),
                "收尾解码",
            )
        }
    }

    /// 收走已就绪的输出帧（解码序——同步解码无重排，顺序不可信）。
    pub(super) fn drain(&self, out: &mut Vec<(Pb, f64)>) {
        while let Ok(item) = self.rx.try_recv() {
            out.push(item);
        }
    }
}

// ————————————————————————————————————————————————————————————
// 播放逐帧解码（§5.3：GPUI 帧流末端的解码出口）
// ————————————————————————————————————————————————————————————

/// 一帧解码输出：NV12 CVPixelBuffer + 源时间轴显示时间。
///
/// CVPixelBuffer 是 CF 对象、引用线程安全（Apple 文档明示可跨线程搬运），
/// 线程绑定声明由本类型给出。`buffer()` 是引用计数克隆（+1）：渲染侧拿
/// 走自己的引用，帧队列里的原件不受影响。
pub struct DecodedFrame {
    inner: Pb,
    /// 该帧的显示时间（秒，源时间轴）。同步解码吐出的是**解码序**，消费方
    /// 必须按这个 PTS 配对显示槽，绝不按吐出顺序排位（与转码链 run() 同一
    /// 纪律）。
    pub pts_secs: f64,
}

unsafe impl Send for DecodedFrame {}

impl DecodedFrame {
    /// 像素缓冲（NV12，IOSurface 背板）。返回值持独立引用（+1）。
    pub fn buffer(&self) -> CVPixelBuffer {
        use core_foundation::base::TCFType;
        // 手写 ffi 里是 *mut c_void，core-video 侧是不透明结构指针——同一 CF 对象
        unsafe { CVPixelBuffer::wrap_under_get_rule(self.inner.0 as CVPixelBufferRef) }
    }

    /// 帧分辨率 `(宽, 高)`。
    pub fn dims(&self) -> (usize, usize) {
        self.inner.dims()
    }
}

impl Clone for DecodedFrame {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            pts_secs: self.pts_secs,
        }
    }
}

// 手写而非 derive：帧的身份是「分辨率 + 显示时间」，指针地址对日志没有意义，
// 而事件枚举要能 Debug 打印（宿主排查用）。
impl std::fmt::Debug for DecodedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.dims();
        f.debug_struct("DecodedFrame")
            .field("dims", &format_args!("{w}x{h}"))
            .field("pts_secs", &self.pts_secs)
            .finish()
    }
}

/// HEVC / H.264 逐帧解码器（播放末端；转码链走 [`super::transcode_h264`]，
/// 不经这里）。
///
/// 同步解码：[`FrameDecoder::decode`] 返回即该样本的全部输出已就绪。
/// 帧经 NV12 CVPixelBuffer（IOSurface 背板）交付，GPU 可直接采样——
/// GPUI 的 `surface()` 元素零拷贝上屏就吃这个形态。
///
/// `full_range = true` 时请求 FullRange NV12（'420f'）：GPU 合成器的
/// YCbCr shader 断言全区间；转码链保持 '420v' 不受影响。
pub struct FrameDecoder {
    inner: Decompressor,
    /// 420f 转换会话：VT 对部分片源不理会 FullRange 请求（输出仍 '420v'），
    /// 而 GPU 合成器断言全区间——首帧发现格式不符时按需建会话转换。
    full_range_transfer: Option<Transfer>,
    actual_format_logged: bool,
}

// VT 解码会话与格式描述都是 CF 对象，CoreFoundation 引用计数线程安全，
// Apple 文档明示会话可跨线程使用；内部通道（Receiver/Box<Sender>）本身
// Send。跨线程约束（async 任务里持解码器）由本声明给出。
unsafe impl Send for FrameDecoder {}

impl FrameDecoder {
    /// 用参数集（HEVC 的 VPS/SPS/PPS 或 H.264 的 SPS/PPS，裸 NAL）建解码会话。
    pub fn open(
        codec: VideoCodec,
        parameter_sets: &[Vec<u8>],
        full_range: bool,
    ) -> AppResult<Self> {
        Ok(Self {
            inner: unsafe { Decompressor::new(codec, parameter_sets, full_range)? },
            full_range_transfer: None,
            actual_format_logged: false,
        })
    }

    /// 解一个样本（MP4 长度前缀 NAL 原样字节），返回本次产生的帧（解码序，
    /// 各自带 PTS）。无输出（如先行参考帧还没喂够）返回空表。
    pub fn decode(
        &mut self,
        sample: &[u8],
        pts_secs: f64,
        duration_secs: f64,
    ) -> AppResult<Vec<DecodedFrame>> {
        unsafe { self.inner.decode(sample, pts_secs, duration_secs)? };
        let ready = self.take_ready();
        // GPU 合成器（gpui surface）断言 FullRange NV12；VT 对部分片源不理会
        // 请求仍吐 '420v'——发现即经 VTPixelTransferSession 转全区间，绝不把
        // 非全区间帧递给渲染端（那里是硬断言，panic 会把主循环打成僵尸）。
        let mut out = Vec::with_capacity(ready.len());
        for frame in ready {
            out.push(self.enforce_full_range(frame));
        }
        Ok(out)
    }

    /// 冲尾帧并返回遗留输出。会话随即不再可用。
    pub fn finish(&mut self) -> AppResult<Vec<DecodedFrame>> {
        unsafe { self.inner.finish()? };
        let ready = self.take_ready();
        let mut out = Vec::with_capacity(ready.len());
        for frame in ready {
            out.push(self.enforce_full_range(frame));
        }
        Ok(out)
    }

    fn take_ready(&self) -> Vec<DecodedFrame> {
        let mut raw = Vec::new();
        self.inner.drain(&mut raw);
        raw.into_iter()
            .map(|(inner, pts_secs)| DecodedFrame { inner, pts_secs })
            .collect()
    }

    fn enforce_full_range(&mut self, frame: DecodedFrame) -> DecodedFrame {
        let fmt = unsafe { ffi::CVPixelBufferGetPixelFormatType(frame.inner.0) };
        if !self.actual_format_logged {
            self.actual_format_logged = true;
            log::info!(
                "[Platform/vt] 解码输出像素格式: {}",
                if fmt == ffi::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
                    "420f（全区间，直上屏）"
                } else {
                    "非 420f，走全区间转换"
                }
            );
        }
        if fmt == ffi::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
            return frame;
        }
        let DecodedFrame { inner, pts_secs } = frame;
        let (w, h) = inner.dims();
        let transfer = self.full_range_transfer.get_or_insert_with(|| {
            // 转换会话只可能因系统资源建不出来；这里失败等于无法渲染，报错终止
            unsafe { Transfer::new(w, h).expect("建全区间转换会话") }
        });
        let converted = unsafe {
            transfer.transfer_to(&inner, ffi::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange)
        };
        match converted {
            Ok(pb) => DecodedFrame {
                inner: pb,
                pts_secs,
            },
            Err(e) => {
                log::warn!("[Platform/vt] 全区间转换失败，保留原帧（渲染端可能拒绝）: {e}");
                DecodedFrame { inner, pts_secs }
            }
        }
    }
}
