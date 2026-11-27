//! 官方收藏夹变更。移动是单次服务端操作，不是删除收藏后重新添加。

use super::model::Entry;
use crate::AppState;
use hongguo_api::{client::ApiEnv, interact};
use hongguo_common::{AppError, AppResult};
use std::collections::HashSet;

pub async fn update_collection_folder_settings(
    state: AppState,
    folder_id: i64,
    name: String,
    publish: bool,
) -> AppResult<()> {
    let env = folder_mutation_env(&state)?;
    interact::update_folder_settings(folder_id, &name, publish, &env).await
}

pub async fn delete_collection_folder(state: AppState, folder_id: i64) -> AppResult<()> {
    let env = folder_mutation_env(&state)?;
    interact::delete_folder(folder_id, &env).await
}

pub async fn move_collection_items(
    state: AppState,
    source_folder_id: i64,
    destination_folder_id: i64,
    entries: Vec<Entry>,
) -> AppResult<()> {
    let env = folder_mutation_env(&state)?;
    let items = move_items(source_folder_id, entries)?;
    interact::move_folder_items(source_folder_id, destination_folder_id, &items, &env).await
}

fn move_items(source: i64, entries: Vec<Entry>) -> AppResult<Vec<interact::FolderMoveItem>> {
    let mut seen = HashSet::new();
    let mut items = Vec::with_capacity(entries.len());
    for entry in entries {
        // Only a row from the original folder carries this ID. In particular, do not
        // substitute the canonical playback ID or silently move just a subset.
        if entry.collection_folder_id != Some(source) {
            return Err(AppError::InvalidArgs("所选条目不属于来源收藏夹".into()));
        }
        let item_id = entry
            .collection_item_id
            .ok_or_else(|| AppError::InvalidArgs("收藏条目缺少来源收藏夹的条目 ID".into()))?;
        if seen.insert(item_id.clone()) {
            items.push(interact::FolderMoveItem {
                item_id,
                book_type: entry.collection_book_type,
                content_type: entry.content_type,
            });
        }
    }
    Ok(items)
}

fn folder_mutation_env(state: &AppState) -> AppResult<ApiEnv> {
    let settings = state.settings();
    if settings.account.is_none() {
        return Err(AppError::Auth("登录后可编辑收藏夹".into()));
    }
    Ok(state.api_env_from_settings(settings))
}
