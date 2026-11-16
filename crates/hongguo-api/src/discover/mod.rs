//! 发现类接口：推荐信息流（榜单落地页 landpage，必须 POST）与找剧筛选。
//! 业务参数全部在 POST body（`genre` 取值 short_play/comic_series/ai_series，
//! 服务端过滤——条目自带的 content_type 数字不可靠）。

mod model;
mod parse;

use serde_json::Value;

use super::client::{ApiEnv, api_call_full};
use hongguo_common::{AppError, AppResult};
use hongguo_signer::API_ORIGIN;

pub use model::{BrowseFilters, FeedItem, FeedPage, SelectorRow};
// 保持公共路径 `discover::SelectorItem` 可用；公开再导出后 rustc 不再按 unused_imports 处理
pub use model::SelectorItem;
// 再导出 utils::json 助手，保持 `super::discover::X` 的既有跨域引用路径不变
pub(crate) use hongguo_common::utils::json::{
    check_code, int_field, num_field, parse_tags, str_field,
};

use parse::{parse_browse_panel, parse_feed};

/// 推荐信息流落地页。
pub const LANDPAGE_PATH: &str = "/reading/distribution/category/landpage/v1/";

// 首页推荐流已迁移到书城换一换（recommend 模块）；landpage 只服务找剧筛选浏览

/// 拉找剧筛选面板（八行维度选项，选项表随服务端运营变化，不落死）。
/// 请求体只有三个键，响应 `data.selector_rows[]`。
pub async fn fetch_browse_panel(env: &ApiEnv) -> AppResult<Vec<SelectorRow>> {
    let body = serde_json::to_vec(&serde_json::json!({
        "need_selector_panel": false,
        "req_scene": "default",
        "req_type": "only_panel",
    }))
    .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = api_call_full(API_ORIGIN, LANDPAGE_PATH, Some(body), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    check_code(&value)?;
    let data = value
        .get("data")
        .ok_or_else(|| AppError::Media("面板响应缺少 data".into()))?;
    parse_browse_panel(Some(data))
}

/// 拉一页找剧结果（与推荐流同端点，多维 select_items 服务端过滤）。
/// `session_id` 首页空串、翻页传上一页响应里的值；`creation_status` 仅选中时在参，全部选省略整个键。
pub async fn fetch_browse(
    filters: &BrowseFilters,
    offset: i64,
    session_id: &str,
    env: &ApiEnv,
) -> AppResult<FeedPage> {
    let mut body = serde_json::json!({
        "client_req_type": 3,
        "filter_ids": "",
        "limit": 20,
        "need_selector_panel": false,
        "offset": offset,
        "req_scene": "default",
        "req_type": "only_content",
        "select_items": filters.to_select_items(),
        "session_id": session_id,
    });
    if !filters.creation_status.is_empty() {
        body["creation_status"] = serde_json::Value::String(filters.creation_status.clone());
    }
    let body = serde_json::to_vec(&body).map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = api_call_full(API_ORIGIN, LANDPAGE_PATH, Some(body), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    check_code(&value)?;
    parse_feed(value.get("data"))
}
