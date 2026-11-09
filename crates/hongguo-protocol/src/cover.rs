//! 封面/头像转码供给（`hongguo-cover://`）：红果条目封面全是 HEIC（WebView 需 HEVC 扩展才能直渲），后端收拢「下载 HEIC → 转 JPEG」，产物按 URL 哈希落盘缓存，同一张只转一次。
//! 评论/剧评头像等「格式说不清」的地址走魔数嗅探：已是 jpeg/png/webp 原样透传，真是 HEIC 才进转码阶梯。

use base64::Engine;
use sha2::{Digest, Sha256};

use hongguo_common::utils::hex;

use hongguo_stream::range::ProtocolResponse;

/// 供给封面/头像转码，返回 `(HTTP 状态码, 响应头, 响应体)`；任何失败返回 `Err`（协议层回 404，前端 `<img>` onError 落占位图）。
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

    for (ext, ct) in [JPEG_KIND, PNG_KIND, WEBP_KIND] {
        if let Ok(bytes) = hongguo_store::paths::qa_read(&cache_path(&remote, ext)) {
            return Ok((200, headers(ct, bytes.len()), bytes));
        }
    }

    let (bytes, ext, ct) = acquire(&remote).map_err(|e| {
        log::warn!("[Cover] 封面/头像获取失败: {e} ({remote})");
        e
    })?;
    // cover-cache 目录没有任何启动/清理路径会创建，写盘前必须先 ensure
    ensure_cache_dir()?;
    // 先写临时文件再改名：转一半的图不该被缓存住
    let final_path = cache_path(&remote, ext);
    let tmp = cache_tmp(&remote);
    if let Err(e) = hongguo_store::paths::qa_write(&tmp, &bytes) {
        let msg = format!("写封面缓存失败: {e}");
        log::warn!("[Cover] {msg}");
        return Err(msg);
    }
    let _ = hongguo_store::paths::qa_rename(&tmp, &final_path);
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
    hongguo_store::paths::cover_cache_dir().join(format!("{digest}.{ext}"))
}

/// 写入中的临时文件（与最终扩展名无关，rename 时才落定为最终名）。
fn cache_tmp(remote: &str) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(remote.as_bytes());
    let digest = hex::encode(&hasher.finalize());
    hongguo_store::paths::cover_cache_dir().join(format!("{digest}.tmp"))
}

fn headers(ct: &str, len: usize) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), ct.into()),
        ("Content-Length".into(), len.to_string()),
        // 内容按 URL 寻址且落了盘缓存，WebView 会话内可以放心复用
        ("Cache-Control".into(), "max-age=604800".into()),
    ]
}

/// 取回「必定可渲染」的封面字节（非 WebView 壳的公开入口）：磁盘缓存当源头，未命中才走 [`serve`] 同一条阶梯，转完落盘——同一张封面全进程只转一次，重启也在。
pub fn acquire_renderable(remote: &str) -> Result<(Vec<u8>, &'static str), String> {
    if let Some((bytes, ct)) = cached_renderable(remote) {
        return Ok((bytes, ct));
    }
    let (bytes, ext, ct) = acquire(remote)?;
    ensure_cache_dir()?;
    let final_path = cache_path(remote, ext);
    let tmp = cache_tmp(remote);
    if let Err(error) = hongguo_store::paths::qa_write(&tmp, &bytes) {
        let msg = format!("写封面缓存失败: {error}");
        log::warn!("[Cover] {msg}");
        return Err(msg);
    }
    let _ = hongguo_store::paths::qa_rename(&tmp, &final_path);
    Ok((bytes, ct))
}

/// 磁盘缓存里已转码好的封面字节与类型；内存解码结果被逐出后从这里重新取字节再解码，本地读比重新下载转码快一个量级。
pub fn cached_renderable(remote: &str) -> Option<(Vec<u8>, &'static str)> {
    for (ext, ct) in [JPEG_KIND, PNG_KIND, WEBP_KIND] {
        if let Ok(bytes) = hongguo_store::paths::qa_read(&cache_path(remote, ext)) {
            return Some((bytes, ct));
        }
    }
    None
}

/// 卡片档长边上限（像素）。缩放必须在落盘前做完——gpui 的 `img()` 不缩放，数据目录里存的就是显示尺寸的图。
pub const EDGE_CARD: u32 = 320;
/// 头像档（评论/回复头像只画 24–44px 的圆）。
pub const EDGE_AVATAR: u32 = 64;
/// 全屏占位档（舞台占位按窗口像素铺，1080p 屏上有一千多 px 宽）。
pub const EDGE_FULL: u32 = 1024;

/// 某一档封面在磁盘上的路径（`<sha256>-<edge>.jpg`）；纯函数，文件未就绪时调用方也能先算出路径当来源。
pub fn sized_cache_path(remote: &str, edge: u32) -> std::path::PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(remote.as_bytes());
    let digest = hex::encode(&hasher.finalize());
    hongguo_store::paths::cover_cache_dir().join(format!("{digest}-{edge}.jpg"))
}

/// 确保某一档封面在磁盘上并返回路径：磁盘命中直接返回，否则下载 → HEIC 转码 → 缩到该档尺寸 → 落盘。
pub fn ensure_sized_cached(remote: &str, edge: u32) -> Result<std::path::PathBuf, String> {
    let path = sized_cache_path(remote, edge);
    if let Ok(bound) = hongguo_store::paths::prepare_qa_io(&path) {
        if hongguo_store::paths::qa_read(&bound).is_ok() {
            return Ok(bound);
        }
    }
    let (bytes, _) = acquire_renderable(remote)?;
    let sized = resize_to(&bytes, edge)?;
    ensure_cache_dir()?;
    let tmp = path.with_extension("tmp");
    if let Err(error) = hongguo_store::paths::qa_write(&tmp, &sized) {
        let msg = format!("写封面缓存失败: {error}");
        log::warn!("[Cover] {msg}");
        return Err(msg);
    }
    let _ = hongguo_store::paths::qa_rename(&tmp, &path);
    Ok(path)
}

/// 卡片档 JPEG 质量（85：肉眼无损，体积比 95 小一半）。
const CARD_JPEG_QUALITY: u8 = 85;

/// 把转码产物缩到指定长边并重编码为 JPEG。
fn resize_to(bytes: &[u8], edge: u32) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;

    let decoded = image::load_from_memory(bytes).map_err(|e| format!("封面解码失败: {e}"))?;
    let resized = if decoded.width().max(decoded.height()) > edge {
        decoded.resize(edge, edge, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    let rgb = resized.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, CARD_JPEG_QUALITY)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("封面编码失败: {e}"))?;
    Ok(out)
}

/// 缓存目录就位（每次写盘前查一次，存在时是一次 exists 开销）。
fn ensure_cache_dir() -> Result<(), String> {
    let dir = hongguo_store::paths::cover_cache_dir();
    hongguo_store::paths::qa_create_dir_all(&dir).map_err(|e| {
        let msg = format!("创建封面缓存目录失败: {e}");
        log::warn!("[Cover] {msg}");
        msg
    })
}

/// 取回「必定可渲染」的字节：先嗅探透传，不行再进 HEIC 转码阶梯。
/// 返回 `(字节, 缓存扩展名, Content-Type)`。
fn acquire(remote: &str) -> Result<(Vec<u8>, &'static str, &'static str), String> {
    let mut downloaded: Option<Vec<u8>> = None;

    // 0) 魔数嗅探透传：已经是 WebView 可直接渲染的格式就不折腾
    //    （无扩展名头像多为 jpeg）
    if let Some((ext, ct)) = sniff_kind(bytes_of(&mut downloaded, remote)?) {
        let bytes = downloaded.take().expect("嗅探刚填充过 downloaded");
        return Ok((bytes, ext, ct));
    }

    // 1) 平台级：封面静图解码（Windows 走 WIC，HEIF/HEVC 扩展缺失会缓存
    //    「不可用」、「重新检测」清缓存后重试；其余平台返回 None 掉级，调用方不需要平台分支）
    if let Some(result) =
        hongguo_media_platform::platform::cover::heic_to_jpeg(bytes_of(&mut downloaded, remote)?)
    {
        match result {
            Ok(bytes) => return Ok((bytes, JPEG_KIND.0, JPEG_KIND.1)),
            Err(msg) => log::warn!("[Cover] WIC 平台解码失败，落 ffmpeg/软解: {msg}"),
        }
    }

    // 2) ffmpeg 级：直接吃 http 输入，一步到位
    if let Some(ffmpeg) = hongguo_media_platform::ffmpeg::probe::ffmpeg_path() {
        // ffmpeg 自己不会建目录：tmp 落在 cover-cache 里，必须先 ensure
        ensure_cache_dir()?;
        let tmp = cache_tmp(remote);
        let mut cmd = std::process::Command::new(ffmpeg);
        cmd.args([
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
        .arg(&tmp);
        // HEIC 封面转码在首页加载期就会跑：GUI 进程里控制面子进程会闪黑窗
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output().map_err(|e| format!("启动 ffmpeg 失败: {e}"))?;
        if out.status.success() {
            return hongguo_store::paths::qa_read(&tmp)
                .map(|bytes| (bytes, JPEG_KIND.0, JPEG_KIND.1))
                .map_err(|e| format!("读取转码产物失败: {e}"));
        }
        let _ = hongguo_store::paths::qa_remove_file(&tmp);
        log::warn!(
            "[Cover] ffmpeg 转码失败，落纯软解: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    // 3) 纯软解级：HEIF 解析 + rusty_h265 + JPEG，零外部依赖
    let heic = bytes_of(&mut downloaded, remote)?;
    let jpeg = hongguo_media::heif::decode_primary_to_jpeg(heic)
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

/// 封面下载超时（秒）：封面链路对延迟敏感，超时即放弃走占位图。
const DOWNLOAD_TIMEOUT_SECS: u64 = 15;

/// 同步 GET。worker 线程上没有 runtime：借用 DB 线程同款 current_thread
/// 内联 runtime 跑一段 async（reqwest 本身是异步 API）。
fn http_get(remote: &str) -> Result<Vec<u8>, String> {
    if hongguo_common::offline_qa::external_url_forbidden(remote) {
        return Err("离线 QA 拒绝外部封面请求".into());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("构建下载 runtime 失败: {e}"))?;
    rt.block_on(async {
        // Android 的 TLS 证书来源分叉，背景见 common::utils::tls
        let client = {
            let builder = reqwest::Client::builder()
                .user_agent("Mozilla/5.0")
                .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS));
            #[cfg(target_os = "android")]
            let builder = builder.use_preconfigured_tls(
                hongguo_common::utils::tls::webpki_tls_config().map_err(|e| e.to_string())?,
            );
            let builder = if hongguo_common::offline_qa::enabled() {
                builder
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::custom(|attempt| {
                        if hongguo_common::offline_qa::parsed_url_forbidden(attempt.url()) {
                            attempt.error(std::io::Error::other("离线 QA 拒绝重定向到外部地址"))
                        } else {
                            attempt.follow()
                        }
                    }))
            } else {
                builder
            };
            builder
                .build()
                .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?
        };
        let resp = client
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
    use super::http_get;
    use super::sniff_kind;
    use super::{EDGE_AVATAR, EDGE_CARD, sized_cache_path};

    #[test]
    fn offline_cover_download_returns_before_http() {
        let previous = std::env::var_os(hongguo_common::offline_qa::ENV);
        unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, "1") };
        let err = http_get("https://example.com/a.heic").expect_err("封面客户端必须先拒绝");
        assert!(err.contains("离线 QA"));
        let err = http_get("http://localhost:80@example.com/a.jpg").expect_err("伪 host");
        assert!(err.contains("离线 QA"));
        match previous {
            Some(value) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, value) },
            None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
        }
    }

    #[test]
    fn sized_cache_path_is_edge_scoped() {
        let url = "https://example.com/a.jpeg?sig=1";
        let card = sized_cache_path(url, EDGE_CARD);
        let avatar = sized_cache_path(url, EDGE_AVATAR);
        assert_ne!(
            card, avatar,
            "档位必须参与寻址——错配就是取不到文件（头像空白）"
        );
        assert!(card.to_string_lossy().ends_with("-320.jpg"));
        assert!(avatar.to_string_lossy().ends_with("-64.jpg"));
    }

    #[test]
    fn jpeg_magic() {
        assert_eq!(
            sniff_kind(&[0xFF, 0xD8, 0xFF, 0xE0, 1, 2]),
            Some(("jpg", "image/jpeg"))
        );
    }

    #[test]
    fn png_magic() {
        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(sniff_kind(&png), Some(("png", "image/png")));
    }

    #[test]
    fn webp_magic() {
        let webp = [
            b'R', b'I', b'F', b'F', 1, 2, 3, 4, b'W', b'E', b'B', b'P', 0,
        ];
        assert_eq!(sniff_kind(&webp), Some(("webp", "image/webp")));
    }

    #[test]
    fn heic_and_garbage_unknown() {
        let heic = [
            0u8, 0, 0, 24, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c',
        ];
        assert_eq!(sniff_kind(&heic), None);
        assert_eq!(sniff_kind(b"not an image"), None);
        assert_eq!(sniff_kind(&[]), None);
    }


    #[test]
    fn qa_cached_renderable_refuses_outside_symlink() {
        let root = std::env::temp_dir().join(format!(
            "hg-cover-read-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let isolated = root.join("isolated");
        let outside = root.join("outside");
        std::fs::create_dir_all(isolated.join("cover-cache")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.jpg");
        std::fs::write(&secret, b"LEAK-BYTES").unwrap();

        let prev_qa = std::env::var_os(hongguo_common::offline_qa::ENV);
        let prev_data = std::env::var_os("HONGGUO_DATA_DIR");
        unsafe {
            std::env::set_var(hongguo_common::offline_qa::ENV, "1");
            std::env::set_var("HONGGUO_DATA_DIR", &isolated);
        }

        let remote = "https://example.com/cover-hit.jpeg";
        let path = super::cache_path(remote, "jpg");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, &path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&secret, &path).unwrap();
        assert!(super::cached_renderable(remote).is_none());
        assert_eq!(std::fs::read(&secret).unwrap(), b"LEAK-BYTES");

        match prev_qa {
            Some(v) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, v) },
            None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
        }
        match prev_data {
            Some(v) => unsafe { std::env::set_var("HONGGUO_DATA_DIR", v) },
            None => unsafe { std::env::remove_var("HONGGUO_DATA_DIR") },
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

