//! 分集列表：调接口 + 解析。分集在响应里的位置固定：
//! `data[series_id].video_data.video_list[]`（不要「递归找第一个数组」——
//! video_data 下还有 celebrities 等数组，猜出来必错且不报错）。

mod model;
mod parse;

use serde_json::Value;

use hongguo_common::{AppError, AppResult};

pub use model::{EpisodeList, RelatedSeries, SeriesCreator, SeriesMeta};
// 保持公共路径 `detail::RelatedItem` 可用；公开再导出后 rustc 不再按 unused_imports 处理
pub use model::RelatedItem;
pub use parse::{parse_episodes, parse_episodes_batch, parse_series_meta};

use parse::parse_related_series;

/// 相关作品端点（详情页「相关推荐」懒加载，lq 域轻签名）：
/// 响应 `data[]` 是 cell 列表，`cell_name:"相关作品"` 的 cell 里 `video_data[]` 带 `tag_info.text` 角标。
pub const PLAN_PATH: &str = "/reading/bookapi/plan/v";

/// 拉一部剧的相关作品·系列。plan/v 首次调用下发冷响应（guess 子格稀少甚至空，连打逐次填充），
/// 故 guess 空时递增退避重试（至多 4 调）取首个非空；全空退最后一次响应。
pub async fn fetch_related_series(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<RelatedSeries> {
    let biz_query: Vec<(String, String)> = [
        ("book_id", series_id),
        ("bookstore_tab", "0"),
        ("bookstore_tab_type", "0"),
        ("current_chapter_num", "0"),
        ("from", "detail_page_more_related"),
        ("is_horizontal_screen", "false"),
        ("limit", "0"),
        ("need_personal_recommend", "1"),
        ("offset", "0"),
        ("post_id", "0"),
        ("scene", "10"),
        ("total_chapter_num", "0"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let mut last = RelatedSeries::default();
    /// 相关推荐的重试次数与递增退避（400/700/1000ms）。
    const PLAN_RETRY_ATTEMPTS: u8 = 4;
    const PLAN_RETRY_BASE_MS: u64 = 400;
    const PLAN_RETRY_STEP_MS: u64 = 300;
    for attempt in 0..PLAN_RETRY_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(
                PLAN_RETRY_BASE_MS + PLAN_RETRY_STEP_MS * (attempt - 1) as u64,
            ))
            .await;
        }
        let bytes = crate::client::api_call_reading(
            crate::danmaku::LQ_API_ORIGIN,
            PLAN_PATH,
            None,
            &biz_query,
            env,
        )
        .await?;

        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
        let related = parse_related_series(&value)?;
        if !related.guess.is_empty() {
            return Ok(related);
        }
        last = related;
    }
    Ok(last)
}

/// 取一部剧的分集列表。
pub async fn fetch_episode_list(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<EpisodeList> {
    let payload = serde_json::to_vec(&crate::params::detail_payload(series_id))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    // sinfoninec 域 + gzip body（sinfonlineb 旧形态已整体回空）
    let bytes = crate::client::api_call_reading(
        crate::client::SINFONLINEC_API_ORIGIN,
        crate::params::DETAIL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    register_episode_types(&value);
    parse_episodes(&value)
}

/// 批量拉多部剧的分集档案（官方预取形态）：一次 `multi_video_detail/preload`，
/// `series_id` 是**逗号串**（官方 6 部一发）；单部残缺由解析层跳过。
pub async fn fetch_episode_lists_batch(
    series_ids: &[String],
    env: &super::client::ApiEnv,
) -> AppResult<Vec<EpisodeList>> {
    let joined = series_ids.join(",");
    let payload = serde_json::to_vec(&crate::params::detail_payload(&joined))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = crate::client::api_call_reading(
        crate::client::SINFONLINEC_API_ORIGIN,
        crate::params::DETAIL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量详情响应失败: {e}")))?;
    register_episode_types(&value);
    parse_episodes_batch(&value)
}

/// 档案响应里的 vid → content_type 进注册表（批量取流分组 key 的来源之一）。
fn register_episode_types(response: &Value) {
    let Some(data) = response.get("data").and_then(Value::as_object) else {
        return;
    };
    for node in data.values() {
        let Some(video_data) = node.get("video_data") else {
            continue;
        };
        let ct = video_data
            .get("content_type")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if ct == 0 {
            continue;
        }
        for ep in video_data
            .get("video_list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if let Some(vid) = ep.get("vid").and_then(Value::as_str) {
                crate::play_url::register_content_type(vid, ct);
            }
        }
    }
}

/// 详情页头部元信息端点：`POST /novel/player/video_detail/v1/`。
/// 追剧数/播放量/季徽/题材标签/备案号都在这里——preload 分集接口不带这些。
pub const VIDEO_DETAIL_PATH: &str = "/novel/player/video_detail/v1/";

/// 拉详情页头部元信息。失败交调用方降级（头部缺这几行不影响主功能）。
pub async fn fetch_series_meta(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<SeriesMeta> {
    // body 照官方（audit 纪律）：screen_width_px 是字符串形态
    let body = serde_json::to_vec(&crate::params::video_detail_payload(series_id))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = super::client::api_call_reading(
        super::client::SINFONLINEC_API_ORIGIN,
        VIDEO_DETAIL_PATH,
        Some(body),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析详情元信息失败: {e}")))?;
    parse_series_meta(&value, series_id)
}
