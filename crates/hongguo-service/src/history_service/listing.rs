use std::{collections::BTreeMap, future::Future};

use hongguo_api::history::{
    WatchHistoryItem, WatchHistoryPage, delete_watch_history, fetch_watch_history,
};
use hongguo_common::{AppError, AppResult};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAccount {
    pub user_id: String,
    pub login_at: i64,
}

pub fn account(state: &AppState) -> Option<HistoryAccount> {
    state.settings().account.map(|account| HistoryAccount {
        user_id: account.user_id,
        login_at: account.login_at,
    })
}

#[derive(Clone, PartialEq, Eq)]
struct Identity(Option<HistoryAccount>);

impl Identity {
    fn capture(state: &AppState) -> Self {
        Self(account(state))
    }

    fn check(&self, state: &AppState) -> AppResult<()> {
        if *self != Self::capture(state) {
            return Err(AppError::Auth("history identity changed".into()));
        }
        Ok(())
    }
}

pub async fn list(
    state: &AppState,
    expected_account: Option<HistoryAccount>,
) -> AppResult<WatchHistoryPage> {
    let identity = Identity(expected_account);
    identity.check(state)?;
    let env = state.api_env();
    identity.check(state)?;
    list_with(state, identity.0, |offset| {
        fetch_watch_history(offset, &env)
    })
    .await
}

async fn list_with<F, Fut>(
    state: &AppState,
    expected_account: Option<HistoryAccount>,
    mut fetch: F,
) -> AppResult<WatchHistoryPage>
where
    F: FnMut(i64) -> Fut,
    Fut: Future<Output = AppResult<WatchHistoryPage>>,
{
    let identity = Identity(expected_account);
    identity.check(state)?;
    let cloud = if identity.0.is_some() {
        collect_pages(|offset| {
            let pending = identity.check(state).map(|()| fetch(offset));
            let identity = &identity;
            async move {
                let result = pending?.await;
                identity.check(state)?;
                result
            }
        })
        .await?
    } else {
        Vec::new()
    };
    identity.check(state)?;
    let local_state = state.clone();
    let local_identity = identity.clone();
    let local = crate::runtime::spawn_blocking(move || {
        local_identity.check(&local_state)?;
        local_state.store.local_history_list()
    })
    .await;
    identity.check(state)?;
    let local =
        local.map_err(|error| AppError::Internal(format!("本地历史读取任务失败: {error}")))??;
    let items = merge(
        cloud,
        local.into_iter().map(WatchHistoryItem::from).collect(),
    );
    Ok(WatchHistoryPage {
        total: items.len() as i64,
        items,
        ..Default::default()
    })
}

async fn collect_pages<F, Fut>(mut fetch: F) -> AppResult<Vec<WatchHistoryItem>>
where
    F: FnMut(i64) -> Fut,
    Fut: Future<Output = AppResult<WatchHistoryPage>>,
{
    let mut offset = 0;
    let mut items = Vec::new();
    loop {
        let page = fetch(offset).await?;
        items.extend(page.items);
        if !page.has_more {
            return Ok(items);
        }
        if page.next_offset <= offset {
            return Err(AppError::Internal(
                "watch history cursor did not advance".into(),
            ));
        }
        offset = page.next_offset;
    }
}

pub async fn delete(
    state: &AppState,
    items: &[(String, String, i64)],
    expected_account: Option<HistoryAccount>,
) -> AppResult<()> {
    let identity = Identity(expected_account);
    identity.check(state)?;
    let env = state.api_env();
    identity.check(state)?;
    delete_with(state, identity.0, items, || {
        delete_watch_history(items, &env)
    })
    .await
}

async fn delete_with<F, Fut>(
    state: &AppState,
    expected_account: Option<HistoryAccount>,
    items: &[(String, String, i64)],
    cloud: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let identity = Identity(expected_account);
    identity.check(state)?;
    if items.is_empty() {
        return Ok(());
    }
    if identity.0.is_some() {
        let result = cloud().await;
        identity.check(state)?;
        result?;
    }
    let series_ids = items
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect::<Vec<_>>();
    let local_state = state.clone();
    let local_identity = identity.clone();
    let result = crate::runtime::spawn_blocking(move || {
        local_identity.check(&local_state)?;
        local_state.store.delete_local_history(&series_ids)
    })
    .await;
    identity.check(state)?;
    result.map_err(|error| AppError::Internal(format!("本地历史删除任务失败: {error}")))?
}

fn merge(cloud: Vec<WatchHistoryItem>, local: Vec<WatchHistoryItem>) -> Vec<WatchHistoryItem> {
    let mut by_series = BTreeMap::<String, WatchHistoryItem>::new();
    for row in cloud.into_iter().chain(local) {
        if by_series
            .get(&row.series_id)
            .is_some_and(|current| current.updated_at_ms > row.updated_at_ms)
        {
            continue;
        }
        by_series.insert(row.series_id.clone(), row);
    }
    let mut items = by_series.into_values().collect::<Vec<_>>();
    for row in &mut items {
        if row.episode_cnt > 0 && row.vid_index > row.episode_cnt {
            row.vid_index = row.episode_cnt;
        }
    }
    items.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.series_id.cmp(&b.series_id))
    });
    items
}

#[cfg(test)]
#[path = "listing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "listing_io_tests.rs"]
mod io_tests;
