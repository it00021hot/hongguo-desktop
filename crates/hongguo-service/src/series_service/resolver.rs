//! 剧集 ID → 完整档案解析。全部走官方 App 接口，失败就报错——不静默返回空列表。
//!
//! ⚠️ 官方接口签名失效时返回 **HTTP 200 + 0 字节**，不是错误码。
//!    所以这里的失败日志必须打印，否则「接口挂了」会表现成「没数据」，
//!    排查方向会被带偏。
//!
//! 输入只收剧集 ID（收藏/历史/详情的档案回退都传 ID）。分享链接解析
//! 已随找剧输入框的链接支持一起下线（2026-10-09）。

use hongguo_common::model::Series;
use hongguo_common::{AppError, AppResult};

/// 解析剧集：输入是纯数字剧集 ID。
pub async fn resolve_series(input: &str, env: &hongguo_api::client::ApiEnv) -> AppResult<Series> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidArgs("请输入剧集 ID".into()));
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::InvalidArgs(
            "无法识别剧集 ID（只收纯数字）".into(),
        ));
    }

    let list = hongguo_api::detail::fetch_episode_list(trimmed, env).await?;
    log::info!(
        "[Series] 官方接口命中《{}》共 {} 集",
        list.title,
        list.episodes.len()
    );
    Ok(series_from_list(list))
}

/// `EpisodeList` → store 的 `Series`（单剧解析与批量预取落库共用）。
pub fn series_from_list(list: hongguo_api::detail::EpisodeList) -> Series {
    Series {
        series_id: list.series_id,
        title: list.title,
        cover: list.cover,
        episode_count: list.episodes.len() as u32,
        followed_cnt: list.followed_cnt,
        content_type: list.content_type,
        episodes: list.episodes,
        tags: Vec::new(),
        dismissed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_input_errors() {
        let env =
            hongguo_api::client::ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        assert!(resolve_series("", &env).await.is_err());
        assert!(resolve_series("   ", &env).await.is_err());
    }

    #[tokio::test]
    async fn non_numeric_input_errors() {
        let env =
            hongguo_api::client::ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        assert!(resolve_series("随便一段文字", &env).await.is_err());
        // 分享链接形态也不再支持：当普通非法输入拒掉
        assert!(
            resolve_series("https://hongguoduanju.com/detail?series_id=7654321", &env)
                .await
                .is_err()
        );
    }
}
