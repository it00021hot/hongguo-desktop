//! 官网取流地址解析。

use crate::error::{AppError, AppResult};

/// 从官网页面里解析视频直链。
pub fn parse_play_url(html: &str) -> AppResult<String> {
    super::extract::first_url(html)
        .filter(|u| u.starts_with("http"))
        .ok_or_else(|| AppError::Media("官网页面里没有视频直链".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_url() {
        let html = r#"{"main":"https://cdn.example.com/v.mp4"}"#;
        assert!(parse_play_url(html).unwrap().ends_with("/v.mp4"));
    }

    #[test]
    fn no_url_errors() {
        assert!(parse_play_url("<html>无</html>").is_err());
    }
}
