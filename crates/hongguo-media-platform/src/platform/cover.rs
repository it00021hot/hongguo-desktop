//! 封面平台级静图解码（HEIC → JPEG）：Windows 走 WIC（HEIF/HEVC 扩展在则用系统解码器），其余平台本层不可用。
//! 全平台同一签名：`None` = 本平台/本层不可用，调用方（协议层封面三级阶梯）掉到下一级即可——**公共调用方因此不需要写任何平台分支**。

/// HEIC 字节 → JPEG。本机没有 HEIF 解码器时返回 `None`（调用方掉级），
/// 有解码器但图像解不开时返回 `Err`（这张图换别的路大概率也不行，但调用方仍会尝试下一级，不赌单个样本）。
#[cfg(target_os = "windows")]
pub fn heic_to_jpeg(heic: &[u8]) -> Option<Result<Vec<u8>, String>> {
    hongguo_media_platform_windows::heic_to_jpeg(heic)
}

/// 非 Windows 平台本层不可用（macOS 的 HEIC 走 ffmpeg/软解级，见
/// `hongguo-protocol` 的封面阶梯）。
#[cfg(not(target_os = "windows"))]
pub fn heic_to_jpeg(_heic: &[u8]) -> Option<Result<Vec<u8>, String>> {
    None
}
