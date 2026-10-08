//! 分享链接 / 文本里的剧集 ID 提取（纯解析，无网络）。

use regex::Regex;

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
