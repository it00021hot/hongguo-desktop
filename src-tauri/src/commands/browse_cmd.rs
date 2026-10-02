//! 浏览与搜索（走内嵌浏览器嗅探）。

use tauri::AppHandle;

use crate::error::AppResult;
use crate::sniff;

/// 浏览分类。
#[tauri::command]
pub fn browse_categories() -> Vec<sniff::Category> {
    sniff::categories()
}

/// 浏览分类页。
///
/// `category` / `genre` 会拼进官网 URL，由嗅探窗口加载后提取卡片。
#[tauri::command]
pub async fn browse_list(
    app: AppHandle,
    category: String,
    genre: Option<String>,
    page: Option<u32>,
) -> AppResult<sniff::SniffResult> {
    // slug 只允许字母数字与连字符，防止拼出意外 URL
    let cat = sanitize_slug(&category, "real-drama");
    let gen = genre
        .as_deref()
        .map(|g| sanitize_slug(g, ""))
        .unwrap_or_default();
    let pg = page.unwrap_or(1).max(1);

    let mut url = format!("{}/category/{cat}", sniff::window::SITE);
    if !gen.is_empty() {
        url.push('/');
        url.push_str(&gen);
    }
    if pg > 1 {
        url.push_str(&format!("?page={pg}"));
    }

    sniff::run(&app, &url)
        .await
        .map_err(crate::error::AppError::Sniff)
}

/// 搜索剧集。
#[tauri::command]
pub async fn search_series(app: AppHandle, keyword: String) -> AppResult<sniff::SniffResult> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Err(crate::error::AppError::InvalidArgs(
            "请输入搜索关键词".into(),
        ));
    }
    let url = format!("{}/search/{}", sniff::window::SITE, urlencoding::encode(kw));
    sniff::run(&app, &url)
        .await
        .map_err(crate::error::AppError::Sniff)
}

/// 清洗 slug：只保留字母数字与连字符。
fn sanitize_slug(input: &str, fallback: &str) -> String {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_keeps_safe_chars() {
        assert_eq!(sanitize_slug("real-drama", ""), "real-drama");
        assert_eq!(sanitize_slug("ai_drama!", ""), "aidrama");
    }

    #[test]
    fn slug_falls_back_on_empty() {
        assert_eq!(sanitize_slug("", "real-drama"), "real-drama");
        assert_eq!(sanitize_slug("///", "x"), "x");
    }

    #[test]
    fn slug_strips_path_traversal() {
        // 不能让输入拼出站外 URL
        let got = sanitize_slug("../../etc/passwd", "");
        assert!(!got.contains('/'));
        assert!(!got.contains('.'));
    }

    #[test]
    fn categories_has_three() {
        assert_eq!(sniff::categories().len(), 3);
    }
}
