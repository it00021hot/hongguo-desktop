use base64::Engine;
use std::path::PathBuf;

const ALLOWED: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v"];

pub fn local_play_url(path: &str) -> Option<String> {
    validate_extension(path).ok()?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path);
    Some(crate::local_url(&encoded))
}

pub fn resolve_path(raw_path: &str) -> Result<PathBuf, &'static str> {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw_path)
        .map_err(|_| "路径编码无效")?;
    let path = String::from_utf8(decoded).map_err(|_| "路径不是合法 UTF-8")?;
    validate_extension(&path)?;
    Ok(PathBuf::from(path))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_url_and_path() {
        let path = "D:\\dl\\剧名 合集.mp4";
        let url = local_play_url(path).expect("allowed video");
        let raw = url.rsplit('/').next().unwrap();
        assert_eq!(resolve_path(raw).unwrap(), PathBuf::from(path));
    }
    #[test]
    fn rejects_non_video_extension() {
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("C:/secret.txt");
        assert_eq!(resolve_path(&raw), Err("只允许播放视频文件"));
    }
    #[test]
    fn rejects_invalid_encoding() {
        assert!(resolve_path("!!!不是 base64!!!").is_err());
    }
    #[test]
    fn play_url_is_none_for_disallowed_extension() {
        assert!(local_play_url("C:/secret.txt").is_none());
    }
}
