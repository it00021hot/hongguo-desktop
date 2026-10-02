//! 浏览与搜索：抓官网页面，解析内嵌数据。
//!
//! 官网是服务端渲染的，卡片、分页、题材全在 `window._ROUTER_DATA` 里，
//! 一次 GET 约 300ms。解析逻辑在 [`super::site::browse`]。

use crate::domain::site::browse::{self, BrowseResult, Category};
use crate::error::{AppError, AppResult};

/// 浏览分类。
#[tauri::command]
pub fn browse_categories() -> Vec<Category> {
    browse::categories()
}

/// 浏览分类页。
///
/// `category` / `genre` 会拼进官网 URL，由抓取层直接解析该页。
#[tauri::command]
pub async fn browse_list(
    category: String,
    genre: Option<String>,
    page: Option<u32>,
) -> AppResult<BrowseResult> {
    let cat = sanitize_slug(&category, "real-drama");
    let gen = genre
        .as_deref()
        .map(|g| sanitize_slug(g, ""))
        .unwrap_or_default();
    let pg = page.unwrap_or(1).max(1);

    let mut url = format!("{}/category/{cat}", browse::SITE);
    if !gen.is_empty() {
        url.push('/');
        url.push_str(&gen);
    }
    if pg > 1 {
        url.push_str(&format!("?page={pg}"));
    }

    browse::load(&url).await
}

/// 搜索剧集。
#[tauri::command]
pub async fn search_series(keyword: String) -> AppResult<BrowseResult> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Err(AppError::InvalidArgs("请输入搜索关键词".into()));
    }
    let url = format!("{}/search/{}", browse::SITE, urlencoding::encode(kw));
    browse::load(&url).await
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
        assert_eq!(browse::categories().len(), 3);
    }
}
