//! 隐藏嗅探窗口的生命周期。
//!
//! 懒创建：首次搜索/浏览时才真正开窗，避免启动就多一个 webview 进程。
//! 窗口默认隐藏并**关闭后台节流**——隐藏的 webview 默认会被降频，
//! 页面加载拖慢会导致嗅探超时。

use std::sync::mpsc;

use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 嗅探窗口的 label。
pub const LABEL: &str = "sniff";

/// 站点 origin。
pub const SITE: &str = "https://hongguoduanju.com";

/// 取（或懒创建）嗅探窗口。
pub fn ensure(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
        return Ok(());
    }

    let url: tauri::Url = SITE.parse().map_err(|e| format!("站点地址无效: {e}"))?;
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("搜索 - 红果短剧")
        .inner_size(1100.0, 820.0)
        .visible(false)
        .background_throttling(BackgroundThrottlingPolicy::Disabled)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// 显示 / 隐藏窗口（超时兜底时让用户自己操作）。
pub fn set_visible(app: &AppHandle, visible: bool) -> Result<(), String> {
    ensure(app)?;
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "嗅探窗口未创建".to_string())?;

    if visible {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    } else {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 在窗口里执行脚本并取回 JSON 结果。
///
/// `eval_with_callback` 把求值结果序列化成 JSON 字符串传给回调。
/// **注意**：Windows 上该 API 会把异常也吞掉，所以传入的脚本必须自己
/// try/catch，并把错误作为字符串返回（不能让它冒出去）。
pub fn eval_json(app: &AppHandle, script: &str) -> Result<String, String> {
    ensure(app)?;
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "嗅探窗口未创建".to_string())?;

    let (tx, rx) = mpsc::channel::<String>();

    window
        .eval_with_callback(script, move |result| {
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;

    // 脚本是同步的，通常几毫秒返回；给足超时避免永久阻塞
    rx.recv_timeout(std::time::Duration::from_secs(20))
        .map(unwrap_result)
        .map_err(|e| format!("脚本未返回结果: {e}"))
}

/// 剥掉 wry 的二次 JSON 编码。
///
/// `eval_with_callback` 会把 JS 返回值再 JSON 编码一次才回调。嗅探脚本用
/// `JSON.stringify` 返回字符串，于是结果被多包一层引号：`[{...}]` 回调回来是
/// `"[{\"...\"}]"`，直接 `from_str` 只会得到 `invalid type: string`。
/// 这里统一还原成脚本原本的 JSON 文本，调用方不必各自记得剥引号。
///
/// 返回值不是 JSON 字符串时（Windows 吞异常的情况）原样透传，由调用方的
/// `from_str` 报出真实解析错误，不在这里猜。
fn unwrap_result(raw: String) -> String {
    match serde_json::from_str::<String>(&raw) {
        Ok(inner) => inner,
        Err(_) => raw,
    }
}

/// 导航到指定 URL。
pub fn navigate(app: &AppHandle, url: &str) -> Result<(), String> {
    ensure(app)?;
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "嗅探窗口未创建".to_string())?;
    let parsed: tauri::Url = url.parse().map_err(|e| format!("URL 无效: {e}"))?;
    window.navigate(parsed).map_err(|e| e.to_string())
}

/// 当前实际加载的地址。站点若把我们重定向到别处，嗅探结果就不作数。
pub fn current_url(app: &AppHandle) -> Option<String> {
    app.get_webview_window(LABEL)
        .and_then(|w| w.url().ok())
        .map(|u| u.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_stable() {
        assert_eq!(LABEL, "sniff");
    }

    #[test]
    fn site_parses_as_url() {
        let url: tauri::Url = SITE.parse().expect("站点地址应合法");
        assert_eq!(url.scheme(), "https");
    }

    #[test]
    fn unwrap_strips_the_extra_json_string_layer() {
        // 脚本返回 JSON.stringify([{...}])，wry 回调时又多包一层引号
        let got = unwrap_result(r#""[{\"a\":1}]""#.to_string());
        assert_eq!(got, r#"[{"a":1}]"#);
        assert!(serde_json::from_str::<Vec<serde_json::Value>>(&got).is_ok());
    }

    #[test]
    fn unwrap_strips_quotes_around_plain_title() {
        // TITLE_JS 返回 document.title，同样是字符串，同样要剥一层
        assert_eq!(unwrap_result("\"红果短剧\"".to_string()), "红果短剧");
    }

    #[test]
    fn unwrap_passes_through_non_string_results() {
        // 异常被 Windows 吞掉时可能不是字符串，原样交给调用方解析报错
        assert_eq!(unwrap_result("null".to_string()), "null");
        assert_eq!(unwrap_result("boom".to_string()), "boom");
    }
}
