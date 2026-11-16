//! 表情域：官方表情 / 第三方 GIF / 自定义表情
//!（口径见 `docs/hongguo-api-endpoints.md` 第 9.5 节）。
//! snake_case 官方协议 ↔ camelCase 对外模型的映射显式写死，不走 serde rename_all。

pub mod model;

pub use model::{
    CommentAttachment, Emoticon, EmoticonCollectRequest, EmoticonPage, EmoticonThumb, PictureUpload,
};

use serde_json::{Map, Value};

use crate::client::ApiEnv;
use crate::params::emoticon::{
    OWN_EMOTICON_KEYWORD, SEARCH_TYPE_MINE, SEARCH_TYPE_SEARCH, SEARCH_TYPE_THIRD_PARTY,
};
use hongguo_common::utils::json::check_code_rejected;
use hongguo_common::{AppError, AppResult};

use super::danmaku::LQ_API_ORIGIN;

/// 表情列表（官方表情 / 第三方 GIF / 自定义表情共用端点）。
const EMOTICON_LIST_PATH: &str = "/reading/ugc/comment/emoticon/v";
/// 图片上传（评论附图与自定义表情共用）。
const PICTURE_UPLOAD_PATH: &str = "/reading/reader/upload/picture/";
/// 收藏/添加自定义表情。
const EMOTICON_COLLECT_PATH: &str = "/reading/ugc/comment/emoticon/collect/v";

/// 拉一页表情。`search_type` 见 [`crate::params::emoticon`]；
/// `keyword` 仅搜索档需要（第三方档传 `None`，我的表情档传 `profile`）。
pub async fn list_emoticons(
    search_type: i64,
    count: i64,
    offset: i64,
    keyword: Option<&str>,
    env: &ApiEnv,
) -> AppResult<EmoticonPage> {
    let mut query = vec![
        ("count".to_string(), count.to_string()),
        ("offset".to_string(), offset.to_string()),
        ("search_type".to_string(), search_type.to_string()),
    ];
    if let Some(kw) = keyword {
        query.push(("keyword".to_string(), kw.to_string()));
    }
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, EMOTICON_LIST_PATH, None, &query, env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析表情列表响应失败: {e}")))?;
    check_code_rejected(&value, "表情操作")?;
    let data = value.get("data").cloned().unwrap_or(Value::Null);
    Ok(EmoticonPage {
        items: data
            .get("info_list")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(parse_emoticon).collect())
            .unwrap_or_default(),
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 关键词搜索表情（`search_type=1`）。`prefix` 为 true 时关键词带 `+`
/// 前缀——官方逐字输入时按前缀匹配（`+c` → `+ca` → `+cat`）。
pub async fn search_emoticons(
    keyword: &str,
    count: i64,
    offset: i64,
    prefix: bool,
    env: &ApiEnv,
) -> AppResult<EmoticonPage> {
    let kw = if prefix {
        format!("+{keyword}")
    } else {
        keyword.to_string()
    };
    list_emoticons(SEARCH_TYPE_SEARCH, count, offset, Some(&kw), env).await
}

/// 第三方表情库（GIF tab，`search_type=2`），按 `offset` 翻页。
pub async fn list_third_party_emoticons(
    count: i64,
    offset: i64,
    env: &ApiEnv,
) -> AppResult<EmoticonPage> {
    list_emoticons(SEARCH_TYPE_THIRD_PARTY, count, offset, None, env).await
}

/// 我上传的自定义表情（♡ tab，`search_type=4&keyword=profile`）。
pub async fn list_own_emoticons(count: i64, offset: i64, env: &ApiEnv) -> AppResult<EmoticonPage> {
    list_emoticons(
        SEARCH_TYPE_MINE,
        count,
        offset,
        Some(OWN_EMOTICON_KEYWORD),
        env,
    )
    .await
}

/// 上传一张图片（评论附图 / 自定义表情共用）。multipart 形态照官方
///（字段名 `picture`、octet-stream、binary、带 Content-Length）；body **不 gzip**。
pub async fn upload_picture(
    bytes: &[u8],
    filename: &str,
    env: &ApiEnv,
) -> AppResult<PictureUpload> {
    let boundary = upload_boundary();
    let body = build_multipart(&boundary, filename, bytes);
    let query = vec![("need_encrypt".to_string(), "0".to_string())];
    let content_type = format!("multipart/form-data; boundary={boundary}");
    let raw = super::client::api_call_reading_raw(
        LQ_API_ORIGIN,
        PICTURE_UPLOAD_PATH,
        body,
        &content_type,
        &query,
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&raw)
        .map_err(|e| AppError::Media(format!("解析图片上传响应失败: {e}")))?;
    check_code_rejected(&value, "表情操作")?;
    Ok(PictureUpload {
        web_uri: value
            .get("web_uri")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        url: value
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 收藏/添加自定义表情（♡ tab「上传自定义表情」与长按「添加到表情」共用）。
/// 请求体是**扁平** JSON（字段见 [`EmoticonCollectRequest`]）；删除（collect_type=2）回 null → 返回 `None`。
pub async fn collect_emoticon(
    req: &EmoticonCollectRequest,
    env: &ApiEnv,
) -> AppResult<Option<Emoticon>> {
    let raw = serde_json::to_vec(req)
        .map_err(|e| AppError::Media(format!("构造收藏表情请求失败: {e}")))?;
    let bytes =
        super::client::api_call_reading(LQ_API_ORIGIN, EMOTICON_COLLECT_PATH, Some(raw), &[], env)
            .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析收藏表情响应失败: {e}")))?;
    check_code_rejected(&value, "表情操作")?;
    Ok(value.pointer("/data/image_data").and_then(parse_emoticon))
}

/// 官方响应条目 → [`Emoticon`]（snake_case 读入）。
pub fn parse_emoticon(value: &Value) -> Option<Emoticon> {
    let web_uri = value.get("web_uri")?.as_str()?.to_string();
    Some(Emoticon {
        id: value.get("id").and_then(Value::as_str).map(str::to_string),
        web_uri,
        dynamic_url: value
            .get("dynamic_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        image_type: value.get("image_type").and_then(Value::as_i64).unwrap_or(0),
        width: value.get("width").and_then(Value::as_i64).unwrap_or(0),
        height: value.get("height").and_then(Value::as_i64).unwrap_or(0),
        format: value
            .get("format")
            .and_then(Value::as_str)
            .map(str::to_string),
        image_name: value
            .get("image_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        thumb_nails: value
            .get("thumb_nails")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|item| EmoticonThumb {
                        web_uri: item
                            .get("web_uri")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        dynamic_url: item
                            .get("dynamic_url")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        width: item.get("width").and_then(Value::as_i64).unwrap_or(0),
                        height: item.get("height").and_then(Value::as_i64).unwrap_or(0),
                        format: item
                            .get("format")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                    .collect()
            }),
        expand_web_url: value
            .get("expand_web_url")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// [`Emoticon`] → 官方 `image_data` 条目（snake_case 写出）。
/// 字段在场性照官方：本地图片只有 web_uri/宽高/format/image_type，表情类另带 id 等。
pub fn image_data_value(image: &Emoticon) -> Value {
    let mut obj = Map::new();
    obj.insert("web_uri".into(), Value::String(image.web_uri.clone()));
    if let Some(id) = &image.id {
        obj.insert("id".into(), Value::String(id.clone()));
    }
    if let Some(dynamic_url) = &image.dynamic_url {
        obj.insert("dynamic_url".into(), Value::String(dynamic_url.clone()));
    }
    obj.insert("image_type".into(), Value::from(image.image_type));
    obj.insert("width".into(), Value::from(image.width));
    obj.insert("height".into(), Value::from(image.height));
    if let Some(format) = &image.format {
        obj.insert("format".into(), Value::String(format.clone()));
    }
    if let Some(image_name) = &image.image_name {
        obj.insert("image_name".into(), Value::String(image_name.clone()));
    }
    if let Some(thumbs) = &image.thumb_nails {
        obj.insert(
            "thumb_nails".into(),
            Value::Array(thumbs.iter().map(thumb_value).collect()),
        );
    }
    if let Some(expand_web_url) = &image.expand_web_url {
        obj.insert(
            "expand_web_url".into(),
            Value::String(expand_web_url.clone()),
        );
    }
    Value::Object(obj)
}

/// 缩略图 → 官方 snake_case 形态。
fn thumb_value(thumb: &EmoticonThumb) -> Value {
    let mut obj = Map::new();
    obj.insert("web_uri".into(), Value::String(thumb.web_uri.clone()));
    obj.insert(
        "dynamic_url".into(),
        Value::String(thumb.dynamic_url.clone()),
    );
    obj.insert("width".into(), Value::from(thumb.width));
    obj.insert("height".into(), Value::from(thumb.height));
    obj.insert("format".into(), Value::String(thumb.format.clone()));
    Value::Object(obj)
}

/// multipart 边界（每次请求唯一，避免与文件内容撞串）。
fn upload_boundary() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("hongguo{nanos}")
}

/// 组装 multipart/form-data 请求体（字段名 `picture`）。
fn build_multipart(boundary: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"picture\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
    body.extend_from_slice(format!("Content-Length: {}\r\n", bytes.len()).as_bytes());
    body.extend_from_slice(b"Content-Transfer-Encoding: binary\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}
