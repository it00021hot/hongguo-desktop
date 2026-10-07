//! 链接 / ID → 分集解析。
//!
//! 先走官方 App 接口，失败再回落官网页面。两级都失败才报错——不静默返回空列表。
//!
//! ⚠️ 官方接口签名失效时返回 **HTTP 200 + 0 字节**，不是错误码。
//!    所以这里的降级日志必须打印，否则「接口挂了」会表现成「官网也没数据」，
//!    排查方向会被带偏。

use crate::domain::model::Series;
use crate::domain::site::series_page::is_renderable_cover;
use crate::error::{AppError, AppResult};

/// 解析剧集：输入可以是分享链接、长文本或纯数字 ID。
pub async fn resolve_series(
    input: &str,
    env: &crate::domain::api::client::ApiEnv,
) -> AppResult<Series> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidArgs("请输入链接或剧集 ID".into()));
    }

    let series_id = crate::domain::site::extract::parse_series_id(trimmed)
        .ok_or_else(|| AppError::InvalidArgs("无法识别剧集 ID".into()))?;

    match crate::domain::api::detail::fetch_episode_list(&series_id, env).await {
        Ok(list) => {
            log::info!(
                "[Series] 官方接口命中《{}》共 {} 集",
                list.title,
                list.episodes.len()
            );
            let mut series = Series {
                series_id: list.series_id,
                title: list.title,
                cover: list.cover,
                episode_count: list.episodes.len() as u32,
                episodes: list.episodes,
                tags: Vec::new(),
                dismissed: false,
            };
            // 接口给的封面是 HEIC，WebView2 解不了（界面上一片裂图），
            // 能拿到官网 webp 版就换。简介与推荐不落盘，由详情页按需提供。
            if !is_renderable_cover(&series.cover) {
                match web_cover(&series_id).await {
                    Some(url) => {
                        log::info!("[Series] 封面换成官网 webp 版");
                        series.cover = url;
                    }
                    None => log::warn!("[Series] 官网也没有 webp 封面，保留接口原图"),
                }
            }
            return Ok(series);
        }
        Err(e) => log::warn!("[Series] 官方接口不可用，改用官网兜底: {e}"),
    }

    let url = format!("https://hongguoduanju.com/detail?series_id={series_id}");
    let html = crate::domain::site::fetch::fetch_site_html(&url).await?;
    crate::domain::site::series_page::parse_series_from_html(&html, &series_id)
}

/// 官网详情页里的可显示封面。
async fn web_cover(series_id: &str) -> Option<String> {
    let url = format!("https://hongguoduanju.com/detail?series_id={series_id}");
    let html = crate::domain::site::fetch::fetch_site_html(&url)
        .await
        .ok()?;
    crate::domain::site::series_page::cover_from_html(&html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_input_errors() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        assert!(resolve_series("", &env).await.is_err());
        assert!(resolve_series("   ", &env).await.is_err());
    }

    #[tokio::test]
    async fn unparsable_input_errors() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        assert!(resolve_series("随便一段文字", &env).await.is_err());
    }
}
