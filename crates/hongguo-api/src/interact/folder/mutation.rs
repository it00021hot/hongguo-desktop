//! Official bookshelf folder writes, kept separate from read and mapping code.

use super::{WATCH, integer, post};
use crate::client::ApiEnv;
use hongguo_common::{AppError, AppResult, utils::time::now_ms};
use serde_json::{Value, json};

const UPDATE_PATH: &str = "/reading/distribution/bookshelf_folder/update/v1/";
const ITEM_UPDATE_PATH: &str = "/reading/distribution/bookshelf_folder/item/update/v1/";
const CREATE: i64 = 1;
const DELETE: i64 = 2;
const UPDATE: i64 = 3;
const MOVE: i64 = 3;
const SHORT_SERIES: i64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderMembership {
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderMoveItem {
    pub item_id: String,
    pub book_type: Option<i64>,
    pub content_type: i64,
}

pub async fn move_folder_items(
    source: i64,
    destination: i64,
    items: &[FolderMoveItem],
    env: &ApiEnv,
) -> AppResult<()> {
    let body = move_payload(source, destination, items, now_ms())?;
    mutate(ITEM_UPDATE_PATH, body, env).await
}

fn move_payload(
    source: i64,
    destination: i64,
    items: &[FolderMoveItem],
    modify_time: i64,
) -> AppResult<Value> {
    if source <= 0 || destination <= 0 || source == destination || items.is_empty() {
        return Err(AppError::InvalidArgs(
            "请选择不同的来源和目标收藏夹及有效条目".into(),
        ));
    }
    let operators = items
        .iter()
        .map(|item| {
            if !item.item_id.parse::<i64>().is_ok_and(|id| id > 0) {
                return Err(AppError::InvalidArgs("收藏条目 ID 无效".into()));
            }
            Ok(json!({
                "operate_type": MOVE,
                "item_id": item.item_id,
                "folder_id": destination,
                "book_type": item.book_type.unwrap_or(WATCH),
                "content_type": if item.content_type > 0 { item.content_type } else { SHORT_SERIES },
                "origin_folder_id": source,
                "modify_time": modify_time,
            }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(json!({"operators": operators}))
}

pub async fn update_folder_items(
    folder: i64,
    action: FolderMembership,
    items: &[(String, i64)],
    env: &ApiEnv,
) -> AppResult<()> {
    if folder <= 0 || items.is_empty() {
        return Err(AppError::Media("未选择收藏夹或内容".into()));
    }
    let operation = match action {
        FolderMembership::Add => 1,
        FolderMembership::Remove => 2,
    };
    let operations = items
        .iter()
        .map(|(id, content_type)| {
            let id = id
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| AppError::Media("收藏条目 ID 无效".into()))?;
            Ok(json!({
                "operate_type": operation,
                "item_id": id.to_string(),
                "folder_id": folder,
                "book_type": 2,
                "content_type": content_type,
            }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    post(ITEM_UPDATE_PATH, json!({"operators": operations}), env).await?;
    Ok(())
}

pub async fn create_folder(name: &str, publish: bool, env: &ApiEnv) -> AppResult<()> {
    let name = valid_folder_name(name)?;
    post(
        UPDATE_PATH,
        json!({
            "operate_data": [{
                "operate_type": CREATE,
                "folder_name": name,
                "is_publish": publish,
            }],
        }),
        env,
    )
    .await?;
    Ok(())
}

pub async fn update_folder_settings(
    id: i64,
    name: &str,
    publish: bool,
    env: &ApiEnv,
) -> AppResult<()> {
    let id = valid_folder_id(id)?;
    let name = valid_folder_name(name)?;
    mutate(
        UPDATE_PATH,
        json!({
            "operate_data": [{
                "operate_type": UPDATE,
                "folder_id": id,
                "folder_name": name,
                "is_publish": publish,
            }],
        }),
        env,
    )
    .await
}

pub async fn delete_folder(id: i64, env: &ApiEnv) -> AppResult<()> {
    let id = valid_folder_id(id)?;
    mutate(
        UPDATE_PATH,
        json!({"operate_data": [{"operate_type": DELETE, "folder_id": id}]}),
        env,
    )
    .await
}

fn valid_folder_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() || name.encode_utf16().count() > 15 {
        return Err(AppError::InvalidArgs(
            "收藏夹名称需为 1 至 15 个字符".into(),
        ));
    }
    Ok(name)
}

fn valid_folder_id(id: i64) -> AppResult<i64> {
    if id <= 0 {
        return Err(AppError::InvalidArgs("收藏夹 ID 无效".into()));
    }
    Ok(id)
}

async fn mutate(path: &str, body: Value, env: &ApiEnv) -> AppResult<()> {
    let response = post(path, body, env).await?;
    if response.get("code").and_then(integer) != Some(0) {
        return Err(AppError::Media("收藏夹操作响应缺少成功状态".into()));
    }
    Ok(())
}
