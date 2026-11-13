//! 探测与进程级缓存：HEVC 解码 / H.264 编码 MediaCodec 是否可建。
//! WebView 直解与转码侧探测分开：软解（google / c2.android）能转码，Chromium 仍解不了。

use parking_lot::RwLock;

use ndk::media::media_codec::{MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;

use super::encode::COLOR_YUV420_SEMIPLANAR;

static HEVC_DEC: RwLock<Option<bool>> = RwLock::new(None);
static HEVC_WEBVIEW: RwLock<Option<bool>> = RwLock::new(None);
static ENC: RwLock<Option<bool>> = RwLock::new(None);
static HW_ENC: RwLock<Option<bool>> = RwLock::new(None);

/// 转码侧：任意 HEVC MediaCodec（含软解）可建即真。
pub fn hevc_decoder_present() -> bool {
    let mut slot = HEVC_DEC.write();
    if slot.is_none() {
        *slot = Some(MediaCodec::from_decoder_type("video/hevc").is_some());
    }
    slot.unwrap_or(false)
}

/// WebView `<video>` 直解是否可信：首选解码器若是系统软解则按不可播（MuMu 等）。
pub fn hevc_webview_decode_available() -> bool {
    let mut slot = HEVC_WEBVIEW.write();
    if slot.is_none() {
        *slot = Some(probe_hevc_webview());
    }
    slot.unwrap_or(false)
}

fn probe_hevc_webview() -> bool {
    let Some(codec) = MediaCodec::from_decoder_type("video/hevc") else {
        log::info!("[Platform/android] HEVC WebView 探测：无解码器");
        return false;
    };
    // getName 需 API 28；更旧的 WebView HEVC 也不稳，直接按不可直解走兼容。
    if android_sdk_int() < 28 {
        log::info!("[Platform/android] HEVC WebView 探测：API<28，按不可直解");
        return false;
    }
    let name = codec.name().unwrap_or_default();
    let soft = is_android_soft_codec_name(&name);
    log::info!("[Platform/android] HEVC WebView 探测 name={name} soft={soft}");
    !soft
}

fn is_android_soft_codec_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("google") || n.starts_with("c2.android.")
}

fn android_sdk_int() -> i32 {
    let mut buf = [0u8; 92];
    let ok = unsafe {
        let key = b"ro.build.version.sdk\0";
        __system_property_get(key.as_ptr().cast(), buf.as_mut_ptr().cast())
    };
    if ok == 0 {
        return 0;
    }
    let s = std::ffi::CStr::from_bytes_until_nul(&buf)
        .ok()
        .and_then(|c| c.to_str().ok())
        .unwrap_or("");
    s.parse().unwrap_or(0)
}

unsafe extern "C" {
    fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> u32;
}

/// H.264 编码会话是否可建（软硬皆可）。
pub fn encoder_available() -> bool {
    let mut slot = ENC.write();
    if slot.is_none() {
        *slot = Some(probe_encoder(false));
    }
    slot.unwrap_or(false)
}

/// 近似「硬编」：优先非 Google / c2.android 软件编码器名。
pub fn h264_hw_encoder_available() -> bool {
    let mut slot = HW_ENC.write();
    if slot.is_none() {
        *slot = Some(probe_encoder(true));
    }
    slot.unwrap_or(false)
}

pub fn clear_probe_cache() {
    *HEVC_DEC.write() = None;
    *HEVC_WEBVIEW.write() = None;
    *ENC.write() = None;
    *HW_ENC.write() = None;
}

fn probe_encoder(prefer_hw: bool) -> bool {
    const SOFT: &[&str] = &["c2.android.avc.encoder", "OMX.google.h264.encoder"];
    if !prefer_hw {
        for name in SOFT {
            if let Some(codec) = MediaCodec::from_codec_name(name)
                && try_configure_encoder(codec)
            {
                return true;
            }
        }
    }
    let Some(codec) = MediaCodec::from_encoder_type("video/avc") else {
        return false;
    };
    if prefer_hw {
        // from_encoder_type 可能仍返回软编；无 MediaCodecList 枚举时只能试配。
        return try_configure_encoder(codec);
    }
    try_configure_encoder(codec)
}

fn try_configure_encoder(codec: MediaCodec) -> bool {
    // SemiPlanar（0x15）在 MuMu 实测可配；Flexible 常失败。
    const COLOR_YUV420_FLEXIBLE: i32 = 0x7f_42_05_00;
    for color in [COLOR_YUV420_SEMIPLANAR, COLOR_YUV420_FLEXIBLE] {
        let mut fmt = MediaFormat::new();
        fmt.set_str("mime", "video/avc");
        fmt.set_i32("width", 256);
        fmt.set_i32("height", 256);
        fmt.set_i32("bitrate", 400_000);
        fmt.set_i32("frame-rate", 30);
        fmt.set_i32("i-frame-interval", 1);
        fmt.set_i32("color-format", color);
        match codec.configure(&fmt, None, MediaCodecDirection::Encoder) {
            Ok(()) => {
                let _ = codec.start();
                let _ = codec.stop();
                log::info!("[Platform/android] H.264 编码探测成功 color={color:#x}");
                return true;
            }
            Err(e) => {
                log::info!("[Platform/android] H.264 编码探测失败 color={color:#x}: {e:?}");
            }
        }
    }
    false
}
