//! HTML 里的字段提取。

use regex::Regex;

/// 从文本里取第一个 `"key":"value"` 形式的字段。
///
/// 用于解析页面内嵌的 JSON 状态（Nuxt/Next 会把数据塞进 script 标签）。
pub fn pick_json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"\\s*:\\s*\"([^\"]*)\"");
    let re = Regex::new(&pattern).ok()?;
    re.captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// 从任意文本里取第一个 URL。
pub fn first_url(text: &str) -> Option<String> {
    // 用 raw string 避免转义地狱：匹配 http(s) 开头、到空白或引号为止
    let re = Regex::new(r#"https?://[^\s"'\\]+"#).ok()?;
    re.find(text).map(|m| m.as_str().to_string())
}

/// 从分享文本里解析 `series_id`。
///
/// 支持 App 分享链接、含引导文案的长文本、纯数字 ID。
pub fn parse_series_id(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }
    let re = Regex::new(r"series_id=(\d{6,})").ok()?;
    re.captures(trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_embedded_json_field() {
        let html = r#"<script>{"title":"我的剧","cover":"x.jpg"}</script>"#;
        assert_eq!(pick_json_string(html, "title").unwrap(), "我的剧");
    }

    #[test]
    fn missing_field_is_none() {
        assert!(pick_json_string("<html></html>", "title").is_none());
    }

    #[test]
    fn finds_first_url() {
        let text = r#"see "https://cdn.example.com/a.mp4" and more"#;
        assert!(first_url(text)
            .unwrap()
            .starts_with("https://cdn.example.com"));
    }

    #[test]
    fn parses_pure_number() {
        assert_eq!(parse_series_id("123456").unwrap(), "123456");
    }

    #[test]
    fn parses_share_link() {
        let link = "【红果短剧】快来看 https://hongguoduanju.com/detail?series_id=7654321 好剧";
        assert_eq!(parse_series_id(link).unwrap(), "7654321");
    }

    #[test]
    fn rejects_short_id() {
        assert!(parse_series_id("series_id=123").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_series_id("随便一段文字").is_none());
        assert!(parse_series_id("").is_none());
    }
}
