//! 链接 / ID → 分集解析。全部走官方 App 接口，失败就报错——不静默返回空列表。
//!
//! ⚠️ 官方接口签名失效时返回 **HTTP 200 + 0 字节**，不是错误码。
//!    所以这里的失败日志必须打印，否则「接口挂了」会表现成「没数据」，
//!    排查方向会被带偏。

use crate::domain::model::Series;
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

    let list = crate::domain::api::detail::fetch_episode_list(&series_id, env).await?;
    log::info!(
        "[Series] 官方接口命中《{}》共 {} 集",
        list.title,
        list.episodes.len()
    );
    Ok(Series {
        series_id: list.series_id,
        title: list.title,
        cover: list.cover,
        episode_count: list.episodes.len() as u32,
        episodes: list.episodes,
        tags: Vec::new(),
        dismissed: false,
    })
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
