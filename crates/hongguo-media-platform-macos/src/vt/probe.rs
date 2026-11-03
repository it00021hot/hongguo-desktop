//! 探测：只回答两个问题——VT 会话能不能建（生产闸门，软编会话也算数）、
//! 是不是硬件（能力徽标与并行度）。结论按进程缓存。

use parking_lot::RwLock;

use hongguo_common::AppResult;

use super::encode::Compressor;
use super::ffi;
use super::pixel::{Pb, iosurface_attrs, os};

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

/// 探测纪律与 ffmpeg 侧同一套：「能建会话」不算数，编码器自己说「我走了硬件」才算。
/// `RequireHardware` 在 Intel Mac 上常见 -12903 误报（明明有 QuickSync 却枚举不到），所以用 `Enable`（允许硬件、不强制）建会话，**实编一帧**后读 `UsingHardwareAcceleratedVideoEncoder`。
/// 探测帧必须用**真实分辨率**（1080p）：VT 按分辨率挑编码器实例，小帧会落到软编实例上、真实会话却是硬件——本机实证过（256×256 报 hw=false，
/// 生产 1080p 会话 CPU 8% 跑 2× 实时/路，明显是硬件）；分辨率下限的教训与 ffmpeg 侧 nvenc（64×64 误判不可用）同源：探测形状要贴近真实使用。
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
        // 逐平面逐行铺 0x80：IOSurface 的整块分配不等于「stride×高×1.5」的纸面加和（平面各自对齐），按整块 memset 实测越界段错误
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
