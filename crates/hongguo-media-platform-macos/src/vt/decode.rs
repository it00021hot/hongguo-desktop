//! HEVC / H.264 解码：同步解码会话（[`Decompressor`]，VTDecompressionSession）。
//!
//! 解码输出回调 [`decode_cb`] 的 refcon 裸指针指向会话内部的 `Box<Sender>`
//! 队列——回调与所属类型必须同文件。

use std::os::raw::c_void;
use std::sync::mpsc::{Receiver, Sender, channel};

use hongguo_common::AppResult;
use hongguo_media_platform_api::video::VideoCodec;

use super::ffi;
use super::pipeline::cm_time_secs;
use super::pixel::{Pb, iosurface_attrs, make_dict, make_int, os};

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
    /// 同步解码会话，输出 NV12（VideoRange '420v'，转码链既定格式）。
    pub(super) unsafe fn new(codec: VideoCodec, parameter_sets: &[Vec<u8>]) -> AppResult<Self> {
        unsafe {
            let label = match codec {
                VideoCodec::Hevc => "HEVC",
                VideoCodec::H264 => "H.264",
            };
            let sizes: Vec<usize> = parameter_sets.iter().map(|n| n.len()).collect();
            let ptrs: Vec<*const u8> = parameter_sets.iter().map(|n| n.as_ptr()).collect();
            let mut format = std::ptr::null_mut();
            // 两种编码的参数集顺序各有讲究：HEVC 要 VPS/SPS/PPS，H.264 要
            // SPS/PPS（提取函数按这个次序给出）。样本长度前缀恒 4
            // （hvcC/avcC 声明的 length_size，提取时已归一校验）。
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

            // 恒按 420v（VideoRange，硬件原生）：曾测过 420f（FullRange）会让
            // VT 在硬解输出后逐帧插入一次色域换算，吞吐掉一个数量级（1080p
            // HEVC 420v 675–785 fps vs 420f 55–67 fps）。转码链重编码前本来
            // 就要做色彩处理，420v 是唯一正确的选择。
            let pixel_format = make_int(ffi::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
            let iosurface = iosurface_attrs();
            // 输出缓冲池最小缓冲数：解码器按它预分配 IOSurface，避免逐帧
            // 新建/回收（帧率高时分配开销直接进帧时延）。6 帧 ≈ 0.2s@30fps，
            // 足够覆盖本引擎 0.4s 前瞻里同时在飞的那批帧。
            const POOL_MIN_BUFFERS: i32 = 6;
            let pool_min = make_int(POOL_MIN_BUFFERS);
            let attrs = make_dict(&[
                (
                    ffi::kCVPixelBufferPixelFormatTypeKey as ffi::CFTypeRef,
                    pixel_format as ffi::CFTypeRef,
                ),
                (
                    ffi::kCVPixelBufferIOSurfacePropertiesKey as ffi::CFTypeRef,
                    iosurface as ffi::CFTypeRef,
                ),
                (
                    ffi::kCVPixelBufferPoolMinimumBufferCountKey as ffi::CFTypeRef,
                    pool_min as ffi::CFTypeRef,
                ),
            ]);

            let (tx, rx) = channel::<(Pb, f64)>();
            let boxed = Box::new(tx);
            let record = ffi::VTDecompressionOutputCallbackRecord {
                callback: Some(decode_cb),
                refcon: &*boxed as *const Sender<(Pb, f64)> as *mut c_void,
            };
            // 显式点名硬件解码：不传规格时 VT 可能落软解，1080p HEVC 软解
            // 46.8ms/帧，转码吞吐直接掉一个量级。
            // 只「建议」不「要求」：个别码流硬解不收时还得让 VT 回软解，
            // 能解总比报错强（建的会话是什么，下面查出来记日志）。
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
            ffi::CFRelease(pool_min as ffi::CFTypeRef);
            os(status, "建解码会话")?;
            // 实时档（RealTime）：让解码器按实时流取舍流水线（见 FFI 声明处
            // 注释）。失败只记日志——属性不被支持时解码仍然正确，只是时延更大。
            let rt = ffi::VTSessionSetProperty(
                session as ffi::VTSessionRef,
                ffi::kVTDecompressionPropertyKey_RealTime,
                ffi::kCFBooleanTrue as ffi::CFTypeRef,
            );
            if rt != ffi::noErr {
                log::warn!("[Platform/vt] 解码器拒收 RealTime（OSStatus {rt}），按默认档继续");
            }
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
            // HEVC 每帧 46.5ms，硬件再快也排不成流水线；异步让硬件并行吃满。
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
