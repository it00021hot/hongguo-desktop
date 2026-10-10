//! 封面/头像转码供给（`hongguo-cover://`）。
//!
//! 红果系的条目封面全是 HEIC（fqnovelpic CDN 签名 URL；改扩展名/换
//! tplv 模板一律 403，签名锁整个 path，2026-10-05 实测），WebView2 只在
//! 装了 HEVC 扩展的机器上能直接渲染。这里把「下载 HEIC → ffmpeg 转
//! JPEG」收进后端：前端把原图 URL base64url 后拼 `{scheme}/c/{…}` 交给
//! `<img>`，产物按 URL 哈希落盘缓存，同一张只转一次。
//!
//! 评论/剧评头像也走这条管线（2026-10-10 起）：无扩展名/`.image` 等
//! 「格式说不清」的地址交给后端**魔数嗅探**——已是 jpeg/png/webp 就
//! 原样透传（不转码、无损），真是 HEIC 才进下面的转码阶梯。
//!
//! 官方客户端（hgplayer）同样要过这一步——它抓包里的封面也是 HEIC，
//! 能显示靠的是客户端自解，不是接口给了别的格式。

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::utils::hex;

use super::range::ProtocolResponse;

/// 供给封面/头像转码。
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

    // 缓存命中：.jpg（转码/历史产物）优先，其次透传的 .png/.webp
    for (ext, ct) in [JPEG_KIND, PNG_KIND, WEBP_KIND] {
        if let Ok(bytes) = std::fs::read(cache_path(&remote, ext)) {
            return Ok((200, headers(ct, bytes.len()), bytes));
        }
    }

    let (bytes, ext, ct) = acquire(&remote).map_err(|e| {
        // 曾几何时这里是静默 404：全机封面挂光日志里一个字都没有。
        // 失败要留痕——URL 哈希级缓存后只发生一次，噪声可控。
        log::warn!("[Cover] 封面/头像获取失败: {e} ({remote})");
        e
    })?;
    // 先写临时文件再改名：转一半的图不该被缓存住
    let final_path = cache_path(&remote, ext);
    let tmp = cache_tmp(&remote);
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写封面缓存失败: {e}"))?;
    let _ = std::fs::rename(&tmp, &final_path);
    Ok((200, headers(ct, bytes.len()), bytes))
}

/// 渲染格式三元组：(缓存扩展名, Content-Type)。
const JPEG_KIND: (&str, &str) = ("jpg", "image/jpeg");
const PNG_KIND: (&str, &str) = ("png", "image/png");
const WEBP_KIND: (&str, &str) = ("webp", "image/webp");

/// 缓存文件路径：`cover-cache/<sha256(url) 全量 32 字节的 hex>.<ext>`。
fn cache_path(remote: &str, ext: &str) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(remote.as_bytes());
    let digest = hex::encode(&hasher.finalize());
    crate::store::paths::cover_cache_dir().join(format!("{digest}.{ext}"))
}

/// 写入中的临时文件（与最终扩展名无关，rename 时才落定为最终名）。
fn cache_tmp(remote: &str) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(remote.as_bytes());
    let digest = hex::encode(&hasher.finalize());
    crate::store::paths::cover_cache_dir().join(format!("{digest}.tmp"))
}

fn headers(ct: &str, len: usize) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), ct.into()),
        ("Content-Length".into(), len.to_string()),
        // 内容按 URL 寻址且落了盘缓存，WebView 会话内可以放心复用
        ("Cache-Control".into(), "max-age=604800".into()),
    ]
}

/// 取回「必定可渲染」的字节：先嗅探透传，不行再进 HEIC 转码阶梯。
/// 返回 `(字节, 缓存扩展名, Content-Type)`。
fn acquire(remote: &str) -> Result<(Vec<u8>, &'static str, &'static str), String> {
    let mut downloaded: Option<Vec<u8>> = None;

    // 0) 魔数嗅探透传：已经是 WebView 可直接渲染的格式就不折腾
    //    （无扩展名头像实测多为 jpeg，2026-10-10 curl 实证）
    if let Some((ext, ct)) = sniff_kind(bytes_of(&mut downloaded, remote)?) {
        let bytes = downloaded.take().expect("嗅探刚填充过 downloaded");
        return Ok((bytes, ext, ct));
    }

    // 1) 平台级：WIC（HEIF/HEVC 扩展在则走系统解码器；扩展没有会缓存
    //    「不可用」，本进程内不再尝试，「重新检测」清缓存）
    #[cfg(target_os = "windows")]
    if let Some(result) =
        crate::media::platform::wic::heic_to_jpeg(bytes_of(&mut downloaded, remote)?)
    {
        match result {
            Ok(bytes) => return Ok((bytes, JPEG_KIND.0, JPEG_KIND.1)),
            Err(msg) => log::warn!("[Cover] WIC 平台解码失败，落 ffmpeg/软解: {msg}"),
        }
    }

    // 2) ffmpeg 级：直接吃 http 输入，一步到位
    if let Some(ffmpeg) = crate::media::ffmpeg::probe::ffmpeg_path() {
        let tmp = cache_tmp(remote);
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
        if out.status.success() {
            return std::fs::read(&tmp)
                .map(|bytes| (bytes, JPEG_KIND.0, JPEG_KIND.1))
                .map_err(|e| format!("读取转码产物失败: {e}"));
        }
        let _ = std::fs::remove_file(&tmp);
        log::warn!(
            "[Cover] ffmpeg 转码失败，落纯软解: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    // 3) 纯软解级：HEIF 解析 + rusty_h265 + JPEG，零外部依赖
    let heic = bytes_of(&mut downloaded, remote)?;
    let jpeg = crate::media::heif::decode_primary_to_jpeg(heic)
        .map_err(|e| format!("HEIC 软解失败: {e}"))?;
    Ok((jpeg, JPEG_KIND.0, JPEG_KIND.1))
}

/// 魔数嗅探：已是 WebView 可直接渲染的格式返回 `(扩展名, Content-Type)`。
fn sniff_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(JPEG_KIND)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(PNG_KIND)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(WEBP_KIND)
    } else {
        None
    }
}

/// 惰性下载：第一级需要字节的才发请求，之后各级复用。
fn bytes_of<'a>(slot: &'a mut Option<Vec<u8>>, remote: &str) -> Result<&'a [u8], String> {
    if slot.is_none() {
        *slot = Some(http_get(remote)?);
    }
    Ok(slot.as_deref().expect("刚填充过"))
}

/// 同步 GET。worker 线程上没有 runtime：借用 DB 线程同款 current_thread
/// 内联 runtime 跑一段 async（reqwest 本身是异步 API）。
fn http_get(remote: &str) -> Result<Vec<u8>, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("构建下载 runtime 失败: {e}"))?;
    rt.block_on(async {
        let resp = reqwest::Client::builder()
            .user_agent("Mozilla/5.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?
            .get(remote)
            .send()
            .await
            .map_err(|e| format!("下载封面失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("封面 CDN 返回 {}", resp.status()));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("读取封面响应失败: {e}"))?;
        Ok(bytes.to_vec())
    })
}

#[cfg(test)]
mod tests {
    use super::sniff_kind;

    #[test]
    fn jpeg_magic() {
        assert_eq!(sniff_kind(&[0xFF, 0xD8, 0xFF, 0xE0, 1, 2]), Some(("jpg", "image/jpeg")));
    }

    #[test]
    fn png_magic() {
        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(sniff_kind(&png), Some(("png", "image/png")));
    }

    #[test]
    fn webp_magic() {
        let webp = [b'R', b'I', b'F', b'F', 1, 2, 3, 4, b'W', b'E', b'B', b'P', 0];
        assert_eq!(sniff_kind(&webp), Some(("webp", "image/webp")));
    }

    #[test]
    fn heic_and_garbage_unknown() {
        // HEIC: ftyp box，major brand heic
        let heic = [0u8, 0, 0, 24, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c'];
        assert_eq!(sniff_kind(&heic), None);
        assert_eq!(sniff_kind(b"not an image"), None);
        assert_eq!(sniff_kind(&[]), None);
    }
}
