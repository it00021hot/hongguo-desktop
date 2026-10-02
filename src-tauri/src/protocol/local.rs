//! 本地成品文件供给（`hongguo-local://`）。
//!
//! 路径不直接进 URL——`file://` 那样的明文路径会被日志与 Referer 带出去，
//! 这里统一用 base64url 编码后放进 `hongguo-local://f/<encoded>`，
//! 服务端再解回来并**校验扩展名**，避免协议被用来读任意文件。

use std::path::PathBuf;

use base64::Engine;

use super::range::{parse_range, partial_headers, ProtocolResponse, RangeSpec};

/// 协议里允许播放的扩展名（小写）。
const ALLOWED: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v"];

/// 供给本地文件。
///
/// 返回 `(HTTP 状态码, 响应头, 响应体)`。
pub fn serve(raw_path: &str, range_header: Option<&str>) -> Result<ProtocolResponse, String> {
    let path = resolve_path(raw_path)?;
    let total = std::fs::metadata(&path)
        .map_err(|e| format!("读取文件信息失败: {e}"))?
        .len();

    let spec = parse_range(range_header, total);
    let mut file = std::fs::File::open(&path).map_err(|e| format!("打开文件失败: {e}"))?;
    use std::io::Read;

    let (status, headers, len) = match spec {
        RangeSpec::Full => {
            seek(&mut file, 0)?;
            (
                200,
                vec![
                    ("Content-Type".into(), content_type(&path).into()),
                    ("Accept-Ranges".into(), "bytes".into()),
                    ("Content-Length".into(), total.to_string()),
                ],
                total,
            )
        }
        RangeSpec::Closed { start, end } => {
            if start >= total {
                return Ok(unsatisfiable(total));
            }
            let end = end.min(total.saturating_sub(1));
            seek(&mut file, start)?;
            let mut headers = vec![("Content-Type".into(), content_type(&path).into())];
            headers.extend(partial_headers(start, end, total));
            (206, headers, end - start + 1)
        }
        RangeSpec::Open { start } => {
            if start >= total {
                return Ok(unsatisfiable(total));
            }
            let end = total.saturating_sub(1);
            seek(&mut file, start)?;
            let mut headers = vec![("Content-Type".into(), content_type(&path).into())];
            headers.extend(partial_headers(start, end, total));
            (206, headers, end - start + 1)
        }
        RangeSpec::Unsatisfiable => return Ok(unsatisfiable(total)),
    };

    let mut body = vec![0u8; len as usize];
    file.read_exact(&mut body)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    Ok((status, headers, body))
}

/// 生成本地播放 URL。路径编码失败或扩展名不允许时返回 `None`。
pub fn local_play_url(path: &str) -> Option<String> {
    validate_extension(path).ok()?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path);
    Some(super::local_url(&encoded))
}

/// 从协议 URL 还原文件路径，并校验扩展名。
pub fn resolve_path(raw_path: &str) -> Result<PathBuf, &'static str> {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw_path)
        .map_err(|_| "路径编码无效")?;
    let path = String::from_utf8(decoded).map_err(|_| "路径不是合法 UTF-8")?;
    validate_extension(&path)?;
    Ok(PathBuf::from(path))
}

/// 只放行视频扩展名，避免协议被用来读任意文件。
fn validate_extension(path: &str) -> Result<(), &'static str> {
    let ext = PathBuf::from(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ALLOWED.contains(&ext.as_str()) {
        Ok(())
    } else {
        Err("只允许播放视频文件")
    }
}

fn seek(file: &mut std::fs::File, pos: u64) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(pos))
        .map(|_| ())
        .map_err(|e| format!("定位失败: {e}"))
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn unsatisfiable(total: u64) -> ProtocolResponse {
    (
        416,
        vec![
            ("Content-Range".into(), format!("bytes */{total}")),
            ("Content-Type".into(), "text/plain".into()),
        ],
        Vec::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_url_and_path() {
        let p = "D:\\dl\\剧名 合集.mp4";
        let url = local_play_url(p).expect("mp4 应允许");
        assert!(url.starts_with("http://hongguo-local.localhost/f/"));
        let raw = url.rsplit('/').next().expect("URL 末段是编码后的路径");
        assert_eq!(resolve_path(raw).unwrap(), PathBuf::from(p));
    }

    #[test]
    fn rejects_non_video_extension() {
        let err =
            resolve_path(&base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("C:/secret.txt"));
        assert_eq!(err, Err("只允许播放视频文件"));
    }

    #[test]
    fn rejects_invalid_encoding() {
        assert!(resolve_path("!!!不是 base64!!!").is_err());
    }

    #[test]
    fn play_url_is_none_for_disallowed_extension() {
        assert!(local_play_url("C:/secret.txt").is_none());
    }

    #[test]
    fn serves_full_file() {
        let dir = std::env::temp_dir().join(format!("hg-local-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.mp4");
        std::fs::write(&p, vec![7u8; 4096]).unwrap();

        let url = local_play_url(p.to_str().unwrap()).unwrap();
        let raw = url.rsplit('/').next().unwrap();
        let (status, headers, body) = serve(raw, None).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body.len(), 4096);
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Accept-Ranges" && v == "bytes"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serves_partial_range() {
        let dir = std::env::temp_dir().join(format!("hg-local-range-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("b.mp4");
        std::fs::write(&p, vec![0u8; 1000]).unwrap();

        let url = local_play_url(p.to_str().unwrap()).unwrap();
        let raw = url.rsplit('/').next().unwrap();
        let (status, headers, body) = serve(raw, Some("bytes=100-199")).unwrap();
        assert_eq!(status, 206);
        assert_eq!(body.len(), 100);
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Content-Range" && v == "bytes 100-199/1000"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_range_start_is_unsatisfiable() {
        let dir = std::env::temp_dir().join(format!("hg-local-oob-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.mp4");
        std::fs::write(&p, vec![0u8; 10]).unwrap();

        let url = local_play_url(p.to_str().unwrap()).unwrap();
        let raw = url.rsplit('/').next().unwrap();
        let (status, _, body) = serve(raw, Some("bytes=500-600")).unwrap();
        assert_eq!(status, 416);
        assert!(body.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
