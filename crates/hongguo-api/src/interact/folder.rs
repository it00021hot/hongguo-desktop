//! 红果收藏夹契约；红果 7.4.3.32 的 DistributionApiService JSON serializer。

use hongguo_common::{AppError, AppResult, utils::json::check_code};
use hongguo_signer::API_ORIGIN;
use serde_json::{Value, json};

use super::list::bookshelf_id;
use crate::client::{ApiEnv, api_call_full};

const LIST_PATH: &str = "/reading/distribution/bookshelf_folder/list/v1/";
const DETAIL_PATH: &str = "/reading/distribution/bookshelf_folder/detail/v1/";
const WATCH: i64 = 2;

mod mutation;

pub use mutation::{
    FolderMembership, FolderMoveItem, create_folder, delete_folder, move_folder_items,
    update_folder_items, update_folder_settings,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BookshelfFolder {
    pub id: i64,
    pub name: String,
    pub item_count: Option<i64>,
    pub published: Option<bool>,
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BookshelfFolderItem {
    pub item_id: String,
    pub series_id: String,
    pub book_type: Option<i64>,
    pub content_type: i64,
    pub title: String,
    pub cover: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BookshelfFolderPage<T> {
    pub items: Vec<T>,
    pub next_offset: Option<i64>,
    pub folder: Option<BookshelfFolder>,
}

pub async fn fetch_folders(
    offset: i64,
    env: &ApiEnv,
) -> AppResult<BookshelfFolderPage<BookshelfFolder>> {
    let response = post(
        LIST_PATH,
        json!({
            "offset": offset,
            "count": 20,
            "preview_items_limit": 4,
            "has_detail": true,
        }),
        env,
    )
    .await?;
    parse_page(&response, "folders", offset, parse_folder)
}

pub async fn fetch_folder_items(
    folder_id: i64,
    offset: i64,
    env: &ApiEnv,
) -> AppResult<BookshelfFolderPage<BookshelfFolderItem>> {
    let response = post(
        DETAIL_PATH,
        json!({
            "folder_id": folder_id,
            "offset": offset,
            "limit": 20,
            "has_detail": true,
        }),
        env,
    )
    .await?;
    let mut page = parse_page(&response, "items", offset, parse_item)?;
    if response.pointer("/data/folder_id").and_then(integer) != Some(folder_id) {
        return Err(AppError::Media("收藏夹详情返回了不同的收藏夹 ID".into()));
    }
    page.folder = response.get("data").and_then(parse_folder);
    Ok(page)
}

async fn post(path: &str, body: Value, env: &ApiEnv) -> AppResult<Value> {
    let raw = serde_json::to_vec(&body).map_err(|error| AppError::Media(error.to_string()))?;
    let bytes = api_call_full(API_ORIGIN, path, Some(raw), &[], env).await?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Media(format!("解析收藏夹响应失败: {error}")))?;
    check_code(&value)?;
    Ok(value)
}

fn parse_page<T>(
    response: &Value,
    items_key: &str,
    offset: i64,
    parse: impl Fn(&Value) -> Option<T>,
) -> AppResult<BookshelfFolderPage<T>> {
    let data = response
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Media("收藏夹响应缺少 data".into()))?;
    let next_offset = if data
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Some(
            data.get("next_offset")
                .and_then(integer)
                .filter(|&next| next > offset)
                .ok_or_else(|| AppError::Media("收藏夹分页偏移未前进".into()))?,
        )
    } else {
        None
    };
    let items = data
        .get(items_key)
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |items| items.iter().filter_map(parse).collect());
    Ok(BookshelfFolderPage {
        items,
        next_offset,
        folder: None,
    })
}

fn parse_folder(raw: &Value) -> Option<BookshelfFolder> {
    let id = raw
        .get("folder_id")
        .and_then(integer)
        .filter(|&id| id > 0)?;
    let name = raw.get("folder_name")?.as_str()?.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let covers = raw
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| cover(item).map(str::to_owned))
        .take(4)
        .collect();
    Some(BookshelfFolder {
        id,
        name,
        covers,
        item_count: raw.get("item_count").and_then(integer),
        published: raw.get("is_publish").and_then(Value::as_bool),
    })
}

fn cover(raw: &Value) -> Option<&str> {
    [
        "/video_detail/series_cover_url",
        "/video_detail/video_data/cover",
    ]
    .into_iter()
    .filter_map(|path| raw.pointer(path).and_then(Value::as_str))
    .find(|cover| !cover.is_empty())
}

fn parse_item(raw: &Value) -> Option<BookshelfFolderItem> {
    let video = raw.get("video_detail").filter(|value| value.is_object());
    let item_id = raw.get("item_id").and_then(bookshelf_id)?;
    let book_type = raw.get("book_type").and_then(integer);
    let series_id = raw
        .pointer("/video_detail/video_data/series_id")
        .and_then(bookshelf_id)
        .or_else(|| {
            // Only a known short-series shelf row can use its item ID as the series ID.
            (book_type == Some(WATCH)).then(|| item_id.clone())
        })?;
    let field = |key| {
        video
            .and_then(|video| video.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some(BookshelfFolderItem {
        item_id,
        series_id,
        book_type,
        content_type: raw
            .get("content_type")
            .and_then(integer)
            .or_else(|| raw.pointer("/video_detail/content_type").and_then(integer))
            .or_else(|| {
                raw.pointer("/video_detail/video_data/content_type")
                    .and_then(integer)
            })
            .unwrap_or_default(),
        title: field("series_title"),
        cover: cover(raw).unwrap_or_default().to_owned(),
    })
}

fn integer(raw: &Value) -> Option<i64> {
    raw.as_i64().or_else(|| raw.as_str()?.parse().ok())
}
