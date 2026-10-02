//! 抓取官网 HTML。

use crate::error::{AppError, AppResult};

/// 官网 UA。
pub const WEB_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

/// 抓取一个页面。
pub async fn fetch_site_html(url: &str) -> AppResult<String> {
    let client = reqwest::Client::builder()
        .user_agent(WEB_UA)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

    let resp = client
        .get(url)
        .header("Referer", "https://hongguoduanju.com/")
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AppError::Network(format!("HTTP {}", resp.status())));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if text.trim().is_empty() {
        return Err(AppError::EmptyResponse("官网返回空页面".into()));
    }
    Ok(text)
}
