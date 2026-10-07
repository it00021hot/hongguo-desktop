//! 封面转码供给（`hongguo-cover://`）。
//!
//! 红果系的条目封面全是 HEIC（fqnovelpic CDN 签名 URL；改扩展名/换
//! tplv 模板一律 403，签名锁整个 path，2026-10-05 实测），WebView2 只在
//! 装了 HEVC 扩展的机器上能直接渲染。这里把「下载 HEIC → ffmpeg 转
//! JPEG」收进后端：前端把原图 URL base64url 后拼 `{scheme}/c/{…}` 交给
//! `<img>`，产物按 URL 哈希落盘缓存，同一张封面只转一次。
//!
//! 官方客户端（hgplayer）同样要过这一步——它抓包里的封面也是 HEIC，
//! 能显示靠的是客户端自解，不是接口给了别的格式。

use base64::Engine;
use sha2::{Digest, Sha256};

use super::range::ProtocolResponse;

/// 供给封面转码。
///
/// 返回 `(HTTP 状态码, 响应头, 响应体)`；任何失败都返回 `Err`（协议层
/// 回 404，前端 `<img>` 的 onError 会落到占位图）。
pub fn serve(raw_path: &str) -> Result<ProtocolResponse, String> {
    let encoded = raw_path
        .trim_start_matches('/')
        .strip_prefix("c/")
        .ok_or_else(|| format!("path 形态不对: {raw_path}"))?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "封面地址不是合法 base64url")?;
    let remote = String::from_utf8(decoded).map_err(|_| "封面地址不是合法 UTF-8")?;
    if !remote.starts_with("https://") {
        return Err("只允许转发 https 封面地址".into());
    }

    let cache = cache_path(&remote);
    if let Ok(bytes) = std::fs::read(&cache) {
        return Ok((200, headers(bytes.len()), bytes));
    }

    let bytes = convert(&remote)?;
    // 先写临时文件再改名：转一半的图不该被缓存住
    let tmp = cache.with_extension("jpg.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写封面缓存失败: {e}"))?;
    let _ = std::fs::rename(&tmp, &cache);
    Ok((200, headers(bytes.len()), bytes))
}

/// 缓存文件路径：`cover-cache/<sha256(url)前16字节hex>.jpg`。
fn cache_path(remote: &str) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(remote.as_bytes());
    let digest = hex(&hasher.finalize());
    crate::store::paths::cover_cache_dir().join(format!("{digest}.jpg"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn headers(len: usize) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), "image/jpeg".into()),
        ("Content-Length".into(), len.to_string()),
        // 封面内容按 URL 寻址且落了盘缓存，WebView 会话内可以放心复用
        ("Cache-Control".into(), "max-age=604800".into()),
    ]
}

/// 下载 HEIC 并转成 JPEG（ffmpeg 直接吃 http 输入，一步到位）。
fn convert(remote: &str) -> Result<Vec<u8>, String> {
    let ffmpeg = crate::media::ffmpeg::probe::ffmpeg_path()
        .ok_or_else(|| "未检测到 ffmpeg，无法转码封面".to_string())?;
    let cache = cache_path(remote);
    let parent = cache
        .parent()
        .ok_or_else(|| "封面缓存路径缺少父目录".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("建封面缓存目录失败: {e}"))?;
    let tmp = cache.with_extension("jpg.tmp");

    let out = std::process::Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            // 网络输入 10 秒读超时（微秒）：CDN 抖动不该把 <img> 挂死
            "-rw_timeout",
            "10000000",
            "-i",
            remote,
            "-frames:v",
            "1",
            "-update",
            "1",
            "-f",
            "image2",
        ])
        .arg(&tmp)
        .output()
        .map_err(|e| format!("启动 ffmpeg 失败: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("封面转码失败: {}", stderr.trim()));
    }
    std::fs::read(&tmp).map_err(|e| format!("读取转码产物失败: {e}"))
}
