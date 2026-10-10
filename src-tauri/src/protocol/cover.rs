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

use crate::utils::hex;

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

    let bytes = convert(&remote).map_err(|e| {
        // 曾几何时这里是静默 404：全机封面挂光日志里一个字都没有。
        // 失败要留痕——URL 哈希级缓存后只发生一次，噪声可控。
        log::warn!("[Cover] 封面转码失败: {e} ({remote})");
        e
    })?;
    // 先写临时文件再改名：转一半的图不该被缓存住
    let tmp = cache.with_extension("jpg.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写封面缓存失败: {e}"))?;
    let _ = std::fs::rename(&tmp, &cache);
    Ok((200, headers(bytes.len()), bytes))
}

/// 缓存文件路径：`cover-cache/<sha256(url) 全量 32 字节的 hex>.jpg`。
fn cache_path(remote: &str) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(remote.as_bytes());
    let digest = hex::encode(&hasher.finalize());
    crate::store::paths::cover_cache_dir().join(format!("{digest}.jpg"))
}

fn headers(len: usize) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), "image/jpeg".into()),
        ("Content-Length".into(), len.to_string()),
        // 封面内容按 URL 寻址且落了盘缓存，WebView 会话内可以放心复用
        ("Cache-Control".into(), "max-age=604800".into()),
    ]
}

/// 下载 HEIC 并转成 JPEG。能力阶梯与兼容合并转码同构：
/// 平台（Windows WIC；macOS 的 WebKit 原生可解、前端根本不进本代理）→
/// ffmpeg → 纯 Rust 软解（[`crate::media::heif`]）——没有 ffmpeg 的机器
/// 封面照常可用，这正是 2026-10-09 全挂事故的修复。
fn convert(remote: &str) -> Result<Vec<u8>, String> {
    let cache = cache_path(remote);
    let parent = cache
        .parent()
        .ok_or_else(|| "封面缓存路径缺少父目录".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("建封面缓存目录失败: {e}"))?;
    let tmp = cache.with_extension("jpg.tmp");

    // WIC 和软解都要先拿到字节；ffmpeg 自己拉 URL。只有真的走到某一级
    // 才下载，下一级复用同一份
    let mut downloaded: Option<Vec<u8>> = None;

    // 1) 平台级：WIC（HEIF/HEVC 扩展在则走系统解码器；扩展没有会缓存
    //    「不可用」，本进程内不再尝试，「重新检测」清缓存）
    #[cfg(target_os = "windows")]
    if let Some(result) =
        crate::media::platform::wic::heic_to_jpeg(bytes_of(&mut downloaded, remote)?)
    {
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(msg) => log::warn!("[Cover] WIC 平台解码失败，落 ffmpeg/软解: {msg}"),
        }
    }

    // 2) ffmpeg 级：直接吃 http 输入，一步到位
    if let Some(ffmpeg) = crate::media::ffmpeg::probe::ffmpeg_path() {
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
            return std::fs::read(&tmp).map_err(|e| format!("读取转码产物失败: {e}"));
        }
        let _ = std::fs::remove_file(&tmp);
        log::warn!(
            "[Cover] ffmpeg 转码失败，落纯软解: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    // 3) 纯软解级：HEIF 解析 + rusty_h265 + JPEG，零外部依赖
    let heic = bytes_of(&mut downloaded, remote)?;
    crate::media::heif::decode_primary_to_jpeg(heic).map_err(|e| format!("HEIC 软解失败: {e}"))
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
