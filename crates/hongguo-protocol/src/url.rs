//! 协议 URL 的拼装与解析。
//!
//! scheme 名（前后端契约）定义在 [`hongguo_common::consts::scheme`]，此处按
//! 本 crate 旧名再导出，使用点沿用 `LOCAL_SCHEME` 等命名不改。

/// 自定义协议 scheme 名（前后端契约）。
///
/// 定义已上移到 `hongguo-common::consts::scheme`（跨 crate 共用）；此处按本
/// 模块旧名再导出，`register.rs` 等沿用 `LOCAL_SCHEME` 等命名不改。
pub use hongguo_common::consts::scheme::{
    COVER as COVER_SCHEME, LOCAL as LOCAL_SCHEME, STREAM as STREAM_SCHEME,
};

/// 自定义协议对外的 URL 前缀（按平台选择形态）。
///
/// **Windows**：WebView2 不支持页面内容直接请求非标准 scheme，wry 靠把
/// `http://{scheme}.…` 开头的请求拦下来再转回自定义协议兜住（`custom_protocol_workaround`），
/// 必须给 `http://{scheme}.localhost/…`；直接给 `hongguo-stream://…` 不会触发
/// 协议处理器，表现就是「后端地址取到了、但永远黑屏、日志里一次请求都没有」。
///
/// **macOS / Linux**：WKWebView（WKURLSchemeHandler）与 webkit2gtk 拦的是
/// **真 scheme**，`http://hongguo-stream.localhost/…` 会被当成真实 HTTP 请求
/// 发往不存在的主机 → `<video>` 直接 MediaError 4。必须给 `{scheme}://localhost/…`（`tauri://localhost`
/// 同款形态，wry 转给 handler 时 host 恒为 localhost）。
pub fn scheme_base(scheme: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("http://{scheme}.localhost")
    } else {
        format!("{scheme}://localhost")
    }
}

/// 在线流 URL。
///
/// 形态按平台走 [`scheme_base`]（Windows = `http://{scheme}.localhost`，
/// macOS/Linux = `{scheme}://localhost`），细节见其文档。
///
/// 档位必须在路径里，有两个作用：`<video>` 靠 URL 变化感知「换了资源」并重新加载；
/// 它同时是缓存的键（见 [`stream`]），所以同一集的不同档位各占一份数据。
pub fn stream_url(vid: &str, definition: u32) -> String {
    format!("{}/s/{definition}/{vid}", scheme_base(STREAM_SCHEME))
}

/// 从在线流 URL 的 path 里取回 `(档位, vid)`。
///
/// 解析不了返回 `None`：path 一定由 [`stream_url`] 生成，对不上就是有别处在乱拼
/// URL，宁可明确报错也不要猜。
pub fn parse_stream_path(path: &str) -> Option<(u32, &str)> {
    let (definition, vid) = path
        .trim_start_matches('/')
        .strip_prefix("s/")?
        .split_once('/')?;
    Some((definition.parse().ok()?, vid))
}

/// 本地成品 URL。`encoded` 是文件路径的 base64url 编码。
///
/// 形态按平台走 [`scheme_base`]，原因见其文档。
pub fn local_url(encoded: &str) -> String {
    format!("{}/f/{encoded}", scheme_base(LOCAL_SCHEME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::local_play_url;

    #[test]
    fn stream_url_uses_the_platform_scheme_form() {
        // Windows 走 http://{scheme}.localhost（WebView2 拦截约定），
        // macOS/Linux 走 {scheme}://localhost（WKURLSchemeHandler/WebKit 拦真 scheme）。
        // 形态给错的表现：<video> 拿到网络错误，MediaError 4 黑屏。
        let url = stream_url("7687919221593885758", 1080);
        let base = scheme_base(STREAM_SCHEME);
        assert_eq!(url, format!("{base}/s/1080/7687919221593885758"));
    }

    #[test]
    fn stream_url_changes_with_definition() {
        // 同一集换清晰度时 URL 必须变，否则 <video> 认为资源没换、不会重新加载
        let base = stream_url("v-42", 1080);
        assert_ne!(base, stream_url("v-42", 720), "换档");
    }

    #[test]
    fn stream_path_survives_the_round_trip() {
        // 请求进来时 wry 已把 scheme 换回自定义协议，host 恒为 localhost，信息在 path 上
        let url = stream_url("v-42", 720);
        let base = scheme_base(STREAM_SCHEME);
        let path = url.strip_prefix(&base).expect("URL 应以 scheme_base 开头");
        assert_eq!(path, "/s/720/v-42");
        assert_eq!(parse_stream_path(path), Some((720, "v-42")));
    }

    #[test]
    fn a_path_we_did_not_generate_is_rejected() {
        // path 只可能由 stream_url 生成，对不上就明确报错，不去猜 vid
        assert_eq!(parse_stream_path("/v-42"), None);
        assert_eq!(parse_stream_path("/s/abc/v-42"), None);
        assert_eq!(parse_stream_path("/s/720"), None);
    }

    #[test]
    fn local_url_uses_the_platform_scheme_form() {
        let url = local_play_url("D:\\a.mp4").unwrap();
        let base = scheme_base(LOCAL_SCHEME);
        assert!(url.starts_with(&format!("{base}/f/")), "实际: {url}");
    }
}
