//! 表情 command：官方表情 / 第三方 GIF / 自定义表情（2026-10-25 官方实包
//! 锁定，口径见 `docs/hongguo-api-endpoints.md` 第 9.5 节）。
//!
//! 能力一一对应官方面板的入口：
//! - GIF tab → `emoticon_list` 的 `thirdParty` 档；
//! - ♡ tab（我的表情）→ `emoticon_list` 的 `own` 档；
//! - ♡ tab 的「+」上传 → `emoticon_upload` + `emoticon_collect`；
//! - 长按表情「添加到表情」→ `emoticon_collect`（带源表情 id）；
//! - 长按自己的表情「删除表情」→ `emoticon_delete`（同一端点，`collect_type=2`）。

use serde::Deserialize;
use tauri::State;

use hongguo_api::emoticon::{
    Emoticon, EmoticonCollectRequest, EmoticonPage, PictureUpload, collect_emoticon,
    list_own_emoticons, list_third_party_emoticons, search_emoticons, upload_picture,
};
use hongguo_api::params::emoticon::{COLLECT_TYPE_ADD, COLLECT_TYPE_DELETE, IMAGE_TYPE_STATIC};
use hongguo_common::AppResult;
use hongguo_service::app_state::AppState;

/// 表情列表档位（与官方 `search_type` 一一对应）。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmoticonSource {
    /// 第三方表情库（GIF tab）。
    ThirdParty,
    /// 我上传的自定义表情（♡ tab）。
    Own,
}

/// 拉表情列表。第三方档按 `offset` 翻页（官方一页 100 条，回 `nextOffset`）。
#[tauri::command]
pub async fn emoticon_list(
    state: State<'_, AppState>,
    source: EmoticonSource,
    count: Option<i64>,
    offset: Option<i64>,
) -> AppResult<EmoticonPage> {
    let env = state.api_env();
    let count = count.unwrap_or(100);
    let offset = offset.unwrap_or(0);
    match source {
        EmoticonSource::ThirdParty => list_third_party_emoticons(count, offset, &env).await,
        EmoticonSource::Own => list_own_emoticons(count, offset, &env).await,
    }
}

/// 关键词搜索表情（面板的搜索 tab）。官方逐字输入按**前缀**匹配
/// （抓包 `+c` → `+ca` → `+cat`），这里照抄该形态。
#[tauri::command]
pub async fn emoticon_search(
    state: State<'_, AppState>,
    keyword: String,
    count: Option<i64>,
    offset: Option<i64>,
) -> AppResult<EmoticonPage> {
    let env = state.api_env();
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Ok(EmoticonPage::default());
    }
    search_emoticons(
        keyword,
        count.unwrap_or(15),
        offset.unwrap_or(0),
        true,
        &env,
    )
    .await
}

/// 上传一张本地图片（评论附图 / 自定义表情共用）。返回服务端给的完整
/// 签名 URL——评论 `image_data.web_uri` 与收藏表情 `image_url` 都引用它。
#[tauri::command]
pub async fn emoticon_upload(
    state: State<'_, AppState>,
    bytes: Vec<u8>,
    filename: String,
) -> AppResult<PictureUpload> {
    let env = state.api_env();
    if bytes.is_empty() {
        return Err(hongguo_common::AppError::Media("图片内容为空".into()));
    }
    upload_picture(&bytes, &filename, &env).await
}

/// 把一张已有表情收藏进「我的表情」（长按表情 → 「添加到表情」）。
///
/// 官方实包（2026-10-25）：`collect_type=1`，`image_id` 填**源表情 id**
/// （收藏别人发的表情即填它的 id；上传本地图片时为空串），`image_url` 填该
/// 表情的 URL（官方取 `dynamic_url ?: web_url ?: web_uri`）。
///
/// 返回服务端落库后的表情对象（带 `id` 与 `expandWebUrl`）——前端直接
/// 插进「我的表情」列表。
///
/// 重复收藏服务端会拒绝（`101066 已添加过该表情`），或回 `code=0` 但
/// `image_data` 为空；两种都按「服务端拒绝」透传原文，不套通用错误前缀。
#[tauri::command]
pub async fn emoticon_collect(
    state: State<'_, AppState>,
    image_id: String,
    image_url: String,
    width: i64,
    height: i64,
    image_type: Option<i64>,
) -> AppResult<Emoticon> {
    let env = state.api_env();
    let req = EmoticonCollectRequest {
        collect_type: COLLECT_TYPE_ADD,
        image_height: height,
        image_id,
        image_type: image_type.unwrap_or(IMAGE_TYPE_STATIC),
        image_url,
        image_width: width,
    };
    collect_emoticon(&req, &env).await?.ok_or_else(|| {
        hongguo_common::AppError::Rejected(
            "服务端未返回表情对象（该表情可能已在我的表情中）".into(),
        )
    })
}

/// 从「我的表情」里删除一个表情（长按自己的表情 → 「删除表情」）。
///
/// 官方实包（2026-10-25）：与添加同一端点，只把 `collect_type` 换成 2；
/// 服务端回 `image_data: null`——没有可回显的表情对象，所以这里只回成功。
#[tauri::command]
pub async fn emoticon_delete(
    state: State<'_, AppState>,
    image_id: String,
    image_url: String,
    width: i64,
    height: i64,
    image_type: Option<i64>,
) -> AppResult<()> {
    let env = state.api_env();
    let req = EmoticonCollectRequest {
        collect_type: COLLECT_TYPE_DELETE,
        image_height: height,
        image_id,
        image_type: image_type.unwrap_or(IMAGE_TYPE_STATIC),
        image_url,
        image_width: width,
    };
    collect_emoticon(&req, &env).await.map(|_| ())
}
