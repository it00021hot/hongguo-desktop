//! 表情域：官方表情 / 第三方 GIF / 自定义表情
//! （口径见 `docs/hongguo-api-endpoints.md` 第 9.5 节）。
//!
//! 三个端点：
//! - `GET /reading/ugc/comment/emoticon/v`——表情列表。`search_type` 分档：
//!   1 = 关键词搜索（`keyword` 带 `+` 前缀表示前缀匹配）、2 = 第三方表情库
//!   （GIF tab，分页 `offset`/`next_offset`）、4 = 我的表情（`keyword=profile`，
//!   官方「♡」档 = 我上传的自定义表情 + 我收藏的官方表情，同一列表）。
//!   3 档官方每次开面板都并发拉，但收藏前后恒空、面板也无入口，
//!   本仓不实现。响应 `data.info_list[]` 为 [`Emoticon`]，另有
//!   `has_more`/`next_offset`；
//! - `POST /reading/reader/upload/picture/`——图片上传（multipart，字段名
//!   `picture`，query `need_encrypt=0`），返回 [`PictureUpload`]；
//! - `POST /reading/ugc/comment/emoticon/collect/v`——添加 / 删除「我的
//!   表情」（`collect_type` 1 = 添加 / 2 = 删除）：上传本地图片存成我的
//!   表情（`image_id` 空串），或把一张已有表情收藏进来（`image_id` 填
//!   源表情 id），响应 `data.image_data` 为 [`Emoticon`]。
//!
//! 官方协议是 snake_case、对外模型是 camelCase——两侧映射在本文件里显式
//! 写死（[`parse_emoticon`] / [`image_data_value`]），不靠 serde 的
//! `rename_all` 一套键名两头通吃。

pub mod model;

pub use model::{
    CommentAttachment, Emoticon, EmoticonCollectRequest, EmoticonPage, EmoticonThumb, PictureUpload,
};

use serde_json::{Map, Value};

use crate::client::ApiEnv;
use crate::params::emoticon::{
    OWN_EMOTICON_KEYWORD, SEARCH_TYPE_MINE, SEARCH_TYPE_SEARCH, SEARCH_TYPE_THIRD_PARTY,
};
use hongguo_common::{AppError, AppResult};

use super::danmaku::LQ_API_ORIGIN;

/// 表情列表（官方表情 / 第三方 GIF / 自定义表情共用端点）。
const EMOTICON_LIST_PATH: &str = "/reading/ugc/comment/emoticon/v";
/// 图片上传（评论附图与自定义表情共用）。
const PICTURE_UPLOAD_PATH: &str = "/reading/reader/upload/picture/";
/// 收藏/添加自定义表情。
const EMOTICON_COLLECT_PATH: &str = "/reading/ugc/comment/emoticon/collect/v";

/// 拉一页表情。
///
/// `search_type` 见 [`crate::params::emoticon`]；`keyword` 仅搜索档需要
/// （第三方档传 `None`，我的表情档传 `profile`）。
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
    check_code(&value)?;
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

/// 上传一张图片（评论附图 / 自定义表情共用）。
///
/// multipart 形态照官方：字段名 `picture`，`Content-Type:
/// application/octet-stream`，`Content-Transfer-Encoding: binary`，
/// 带 `Content-Length`；body **不 gzip**（reading 族其余接口才 gzip）。
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
    check_code(&value)?;
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

/// 把一张已上传的图片存成自定义表情，或把一张已有表情收藏进「我的表情」
/// （♡ tab 的「上传自定义表情」与长按「添加到表情」共用本端点）。
///
/// 请求体是**扁平** JSON（不是 `business_param` 形态），字段见
/// [`EmoticonCollectRequest`]：上传走 `image_id=""` + 上传接口返回的完整
/// 签名 URL；收藏已有表情走 `image_id` = 源表情 id + 该表情的 URL。
///
/// 返回值：添加（`collect_type=1`）服务端回完整表情对象；删除
/// （`collect_type=2`）回 `image_data: null`——此时返回 `None`。
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
    check_code(&value)?;
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
///
/// 字段在场性照官方：本地图片只有 `web_uri`/宽高/`format`/
/// `image_type`，表情类另带 `id`/`dynamic_url`/`thumb_nails` 等。
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

/// 服务端业务拒绝用 `Rejected` 变体透传原文（与互动域同口径，见
/// `interact/mod.rs`）：「已添加过该表情」这类具体原因对用户是有效信息，
/// 套「视频处理失败」这类通用文案反而误导。
fn check_code(value: &Value) -> AppResult<()> {
    if value.get("code").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    let msg = value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/BaseResp/StatusMessage")
                .and_then(Value::as_str)
        })
        .unwrap_or("未知错误")
        .trim();
    Err(AppError::Rejected(format!(
        "表情操作被服务端拒绝（{code}）: {msg}"
    )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_body_matches_official_layout() {
        let body = build_multipart("BOUND", "a.png", b"PNG");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUND\r\n"));
        assert!(
            text.contains("Content-Disposition: form-data; name=\"picture\"; filename=\"a.png\"")
        );
        assert!(text.contains("Content-Type: application/octet-stream"));
        assert!(text.contains("Content-Length: 3"));
        assert!(text.contains("Content-Transfer-Encoding: binary\r\n\r\nPNG"));
        assert!(text.ends_with("\r\n--BOUND--\r\n"));
    }

    #[test]
    fn parses_official_third_party_item() {
        let raw = serde_json::json!({
            "web_uri": "https://p26-im-emoticon-sign.byteimg.com/x~tplv-3jr8j4ixpe-resize:450:450.jpeg?lk3s=1",
            "width": 450, "height": 450, "format": "", "id": "6996174263220784397",
            "dynamic_url": "https://p26-im-emoticon-sign.byteimg.com/x~tplv-3jr8j4ixpe-resize:450:450.gif?lk3s=1",
            "image_type": 2,
            "thumb_nails": [{"web_uri": "", "width": 100, "height": 100, "format": "",
                             "dynamic_url": "https://p26-im-emoticon-sign.byteimg.com/x-100.gif"}],
            "image_name": ""
        });
        let item = parse_emoticon(&raw).unwrap();
        assert_eq!(item.image_type, 2);
        assert_eq!(item.id.as_deref(), Some("6996174263220784397"));
        assert_eq!(item.width, 450);
        assert_eq!(item.format.as_deref(), Some(""));
        assert_eq!(item.image_name.as_deref(), Some(""));
        assert_eq!(item.thumb_nails.as_ref().unwrap().len(), 1);
        assert!(item.dynamic_url.is_some());
    }

    #[test]
    fn local_picture_image_data_omits_absent_fields() {
        // 本地图片评论的 image_data 只有 web_uri/宽高/格式/image_type
        let image = Emoticon {
            web_uri: "https://p0-reading-private.fqnovel.com/x~tplv-s85hriknmn-jpeg.jpeg?lk3s=1"
                .into(),
            width: 1080,
            height: 2340,
            format: Some("jpeg".into()),
            image_type: 1,
            ..Emoticon::default()
        };
        let obj = image_data_value(&image);
        let map = obj.as_object().unwrap();
        assert_eq!(map.len(), 5);
        assert!(map.contains_key("web_uri"));
        assert!(map.contains_key("image_type"));
        assert!(!map.contains_key("id"));
        assert!(!map.contains_key("dynamic_url"));
        assert!(!map.contains_key("thumb_nails"));
    }

    #[test]
    fn third_party_image_data_keeps_official_fields() {
        let raw = serde_json::json!({
            "web_uri": "https://p26-im-emoticon-sign.byteimg.com/x.jpeg",
            "width": 196, "height": 196, "format": "", "id": "6996174263220784397",
            "dynamic_url": "https://p26-im-emoticon-sign.byteimg.com/x.gif",
            "image_type": 2,
            "thumb_nails": [{"web_uri": "", "width": 100, "height": 100, "format": "",
                             "dynamic_url": "https://p26-im-emoticon-sign.byteimg.com/x-100.gif"}],
            "image_name": ""
        });
        let image = parse_emoticon(&raw).unwrap();
        let map = image_data_value(&image).as_object().unwrap().clone();
        assert_eq!(map.len(), 9);
        assert_eq!(map["id"], Value::from("6996174263220784397"));
        assert_eq!(map["image_name"], Value::from(""));
        assert!(map["thumb_nails"].is_array());
        assert!(!map.contains_key("expand_web_url"));
    }

    #[test]
    fn collect_request_matches_official_capture() {
        // 官方形态（长按评论里的 GIF → 「添加到表情」）：
        // image_id 是**源表情 id**，image_url 是该表情的静态 URL，
        // image_type=2（动图）但 URL 仍是静态 webp。
        use crate::params::emoticon::COLLECT_TYPE_ADD;
        let req = EmoticonCollectRequest {
            collect_type: COLLECT_TYPE_ADD,
            image_height: 449,
            image_id: "7305929666710342938".to_string(),
            image_type: 2,
            image_url: "https://p26-im-emoticon-sign.byteimg.com/x~tplv-3jr8j4ixpe-resize:450:450.webp?lk3s=43402efa&format=.webp".to_string(),
            image_width: 425,
        };
        let body = serde_json::to_value(&req).unwrap();
        let map = body.as_object().unwrap();
        assert_eq!(map.len(), 6);
        assert_eq!(body["collect_type"], Value::from(1));
        assert_eq!(body["image_id"], Value::from("7305929666710342938"));
        assert_eq!(body["image_type"], Value::from(2));
        assert_eq!(body["image_width"], Value::from(425));
        assert_eq!(body["image_height"], Value::from(449));
    }

    #[test]
    fn collect_request_keeps_official_field_order() {
        // 官方体按 key 字母序序列化（collect_type → image_height → image_id
        // → image_type → image_url → image_width），本仓结构体按同序声明，
        // 逐字节形态与官方一致。
        let text = serde_json::to_string(&EmoticonCollectRequest::default()).unwrap();
        assert_eq!(
            text,
            r#"{"collect_type":0,"image_height":0,"image_id":"","image_type":0,"image_url":"","image_width":0}"#
        );
    }

    #[test]
    fn server_rejection_passes_through_original_message() {
        // 重复收藏服务端回 101066「已添加过该表情」：原文透传，不套
        // 「视频处理失败」这类通用前缀——前缀会把重复收藏说成视频问题。
        let value = serde_json::json!({"code": 101066, "message": "已添加过该表情 "});
        let err = check_code(&value).unwrap_err();
        assert_eq!(err.i18n_key(), "error.rejected");
        assert!(
            err.to_string().contains("已添加过该表情"),
            "服务端原文应保留: {}",
            err
        );
        assert!(!err.to_string().contains("媒体处理失败"));
    }
}
