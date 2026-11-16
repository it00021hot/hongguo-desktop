//! 媒体来源 URL 的拼装与解析（原 `hongguo-protocol`）：在线流、本地成品的跨端 URL 形态。
//!
//! 只做纯字符串/编解码，不触网络与文件系统；供数本体在 NSH 的本机 HTTP 服务。

use base64::Engine;
use std::path::PathBuf;

/// scheme 名（前后端契约），本体在 [`crate::consts::scheme`]，此处按旧名再导出。
pub use crate::consts::scheme::{LOCAL as LOCAL_SCHEME, STREAM as STREAM_SCHEME};

const ALLOWED: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v"];

/// 自定义协议对外的 URL 前缀，按平台二选一：Windows/Android 必须给 `http://{scheme}.localhost`
/// （WebView2 与 Android WebView 不拦非 http scheme），macOS/Linux 必须给 `{scheme}://localhost`（拦真 scheme，http 形态会被当真实 HTTP 请求发往不存在的主机）。
pub fn scheme_base(scheme: &str) -> String {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        format!("http://{scheme}.localhost")
    } else {
        format!("{scheme}://localhost")
    }
}

/// 在线流 URL，形态按平台走 [`scheme_base`]。档位编码进 path：`<video>` 靠 URL 变化感知换资源并重新加载，它同时是 `hongguo-stream` 缓存的键。
pub fn stream_url(vid: &str, definition: u32) -> String {
    format!("{}/s/{definition}/{vid}", scheme_base(STREAM_SCHEME))
}

/// 从在线流 URL 的 path 里取回 `(档位, vid)`；解析不了返回 `None`——path 只会由 [`stream_url`] 生成，对不上就是别处乱拼 URL，宁可明确报错不猜。
pub fn parse_stream_path(path: &str) -> Option<(u32, &str)> {
    let (definition, vid) = path
        .trim_start_matches('/')
        .strip_prefix("s/")?
        .split_once('/')?;
    Some((definition.parse().ok()?, vid))
}

/// 本地成品 URL。`encoded` 是文件路径的 base64url 编码，形态按平台走 [`scheme_base`]。
fn local_url(encoded: &str) -> String {
    format!("{}/f/{encoded}", scheme_base(LOCAL_SCHEME))
}

/// 本地文件 → 可播放 URL；扩展名不在白名单返回 `None`。
pub fn local_play_url(path: &str) -> Option<String> {
    validate_extension(path).ok()?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path);
    Some(local_url(&encoded))
}

/// 从本地 URL 的 path 段还原文件路径；编码或扩展名不合法返回错误。
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
