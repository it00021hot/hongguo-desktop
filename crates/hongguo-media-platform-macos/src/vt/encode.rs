//! H.264 编码：VTCompressionSession 会话（[`Compressor`]，硬编优先，无硬编
//! 回落 Apple 内置软编会话），输出 AVCC 样本转 Annex-B。
//!
//! 编码输出回调 [`encode_cb`] 的 refcon 裸指针指向会话内部的 `Box<Sender>`
//! 队列——回调与所属类型必须同文件。

use std::os::raw::c_void;
use std::sync::mpsc::{Receiver, Sender, channel};

use hongguo_common::AppResult;

use super::ffi;
use super::pixel::{CmSb, Pb, iosurface_attrs, make_dict, make_int, os};

// ————————————————————————————————————————————————————————————
// 编码
// ————————————————————————————————————————————————————————————

pub(super) struct Compressor {
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
    pub(super) unsafe fn new(width: usize, height: usize, bitrate: i32) -> AppResult<Self> {
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
            // 顺序，pts 标注与封装器的「严格递增」要求都不受干扰。
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
    pub(super) unsafe fn encode(
        &mut self,
        frame: &Pb,
        pts_secs: f64,
        duration_secs: f64,
    ) -> AppResult<()> {
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
    pub(super) unsafe fn finish(&mut self) -> AppResult<()> {
        unsafe {
            os(
                ffi::VTCompressionSessionCompleteFrames(self.session, ffi::kCMTimeInvalid),
                "收尾编码",
            )
        }
    }

    /// 编码器自报是否真走了硬件（`UsingHardwareAcceleratedVideoEncoder`）。
    pub(super) unsafe fn using_hardware(&self) -> AppResult<bool> {
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
    pub(super) fn take_outputs(&mut self) -> Vec<CmSb> {
        let mut out = Vec::new();
        while let Ok(sb) = self.rx.try_recv() {
            out.push(sb);
        }
        out
    }

    /// 把一个编码样本转成封装器要的 Annex-B；返回 `(Annex-B, 是否关键帧)`。
    ///
    /// 关键帧前插 SPS/PPS：拼接器重建 moov 时样本描述取自第 1 集，后续各集
    /// 的码流必须自带参数集才能在第 1 集的 avcC 之下解码。
    pub(super) unsafe fn sample_to_unit(&mut self, sb: &CmSb) -> AppResult<(Vec<u8>, bool)> {
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
            // hongguo_media::hevc 是同一份实现
            let mut annexb = hongguo_media::hevc::to_annexb(raw, 4);

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
/// 按「缺省即非同步」判会把首帧标成非关键帧，封装器直接拒收。
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
