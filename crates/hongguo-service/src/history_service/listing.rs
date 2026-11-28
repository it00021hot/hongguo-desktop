use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
};

use hongguo_api::history::{
    WatchHistoryItem, WatchHistoryPage, delete_watch_history, fetch_watch_history,
    fetch_watch_history_for_verification,
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
    if items.is_empty() {
        return Ok(());
    }
    if identity.0.is_none() {
        log::info!("[History] 未登录，仅删除本地历史 {} 条", items.len());
        return delete_local(state, &identity, items).await;
    }
    let env = state.api_env();
    identity.check(state)?;
    let selected: BTreeSet<&str> = items.iter().map(|(id, _, _)| id.as_str()).collect();
    let fetch = |offset| fetch_watch_history_for_verification(offset, &env);
    let before = collect_pages(fetch).await?;
    let targets = cloud_delete_targets(&before, &selected)?;
    identity.check(state)?;
    if targets.is_empty() {
        return delete_local(state, &identity, items).await;
    }
    let write = delete_watch_history(&targets, &env).await;
    identity.check(state)?;
    let after = collect_pages(fetch).await?;
    identity.check(state)?;
    let still_listed: BTreeSet<&str> = after
        .iter()
        .map(|item| item.series_id.as_str())
        .filter(|id| selected.contains(id))
        .collect();
    if still_listed.is_empty() {
        return delete_local(state, &identity, items).await;
    }
    let confirmed: Vec<_> = items
        .iter()
        .filter(|(id, _, _)| !still_listed.contains(id.as_str()))
        .cloned()
        .collect();
    delete_local(state, &identity, &confirmed).await?;
    write?;
    Err(AppError::Rejected(format!(
        "{} 条观看历史仍在云端，未删除",
        still_listed.len()
    )))
}

fn cloud_delete_targets(
    rows: &[WatchHistoryItem],
    selected: &BTreeSet<&str>,
) -> AppResult<Vec<(String, i64)>> {
    rows.iter()
        .filter(|item| selected.contains(item.series_id.as_str()))
        .map(|item| {
            let kind = item
                .book_type
                .filter(|kind| *kind > 0)
                .ok_or_else(|| AppError::Rejected("该观看记录缺少类型，未删除".into()))?;
            Ok((item.series_id.clone(), kind))
        })
        .collect::<AppResult<BTreeSet<_>>>()
        .map(|rows| rows.into_iter().collect())
}

async fn delete_local(
    state: &AppState,
    identity: &Identity,
    items: &[(String, String, i64)],
) -> AppResult<()> {
    identity.check(state)?;
    if items.is_empty() {
        return Ok(());
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
