use super::model::HistoryItem;
use hongguo_api::history::{self, WatchHistoryItem};
use hongguo_common::{AppError, AppResult};
use hongguo_service::AppState;
use std::{collections::HashSet, future::Future};

pub(super) async fn load(state: AppState) -> AppResult<Vec<HistoryItem>> {
    if state.settings().account.is_none() {
        return hongguo_service::runtime::spawn_blocking(move || {
            state
                .store
                .local_history_list()
                .map(|items| items.into_iter().map(HistoryItem::from).collect())
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    }
    let env = state.api_env();
    collect_pages(|offset| history::fetch_watch_history(offset, &env))
        .await
        .map(|items| items.into_iter().map(HistoryItem::from).collect())
}

async fn collect_pages<F, Fut>(mut fetch: F) -> AppResult<Vec<WatchHistoryItem>>
where
    F: FnMut(i64) -> Fut,
    Fut: Future<Output = AppResult<history::WatchHistoryPage>>,
{
    let mut offset = 0;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let page = fetch(offset).await?;
        items.extend(
            page.items
                .into_iter()
                .filter(|item| seen.insert(item.series_id.clone())),
        );
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

pub(super) async fn remove(
    state: AppState,
    entries: Vec<HistoryItem>,
    expected_user: Option<String>,
) -> AppResult<()> {
    if state
        .settings()
        .account
        .as_ref()
        .map(|account| account.user_id.clone())
        != expected_user
    {
        return Err(AppError::Auth("history identity changed".into()));
    }
    if expected_user.is_none() {
        return hongguo_service::runtime::spawn_blocking(move || {
            state
                .store
                .delete_local_history(&entries.into_iter().map(|item| item.id).collect::<Vec<_>>())
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    }
    let refs = entries
        .into_iter()
        .map(|item| (item.id, item.vid, i64::from(item.episode)))
        .collect::<Vec<_>>();
    history::delete_watch_history(&refs, &state.api_env()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_api::history::WatchHistoryPage;

    #[tokio::test]
    async fn all_pages_preserve_ids_deduplicate_and_accept_empty_intermediate_pages() {
        let calls = std::cell::RefCell::new(Vec::new());
        let rows = collect_pages(|offset| {
            calls.borrow_mut().push(offset);
            std::future::ready(Ok(WatchHistoryPage {
                items: if offset == 1 {
                    Vec::new()
                } else {
                    vec![WatchHistoryItem {
                        series_id: "9007199254740993".into(),
                        ..Default::default()
                    }]
                },
                has_more: offset < 2,
                next_offset: offset + 1,
                total: 1,
            }))
        })
        .await
        .unwrap();
        assert_eq!(*calls.borrow(), [0, 1, 2]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].series_id, "9007199254740993");
    }

    #[tokio::test]
    async fn stalled_cursor_and_page_errors_do_not_report_partial_success() {
        assert!(
            collect_pages(|_| std::future::ready(Ok(WatchHistoryPage {
                has_more: true,
                ..Default::default()
            })))
            .await
            .is_err()
        );
        assert!(
            collect_pages(|_| std::future::ready(Err(AppError::Io("offline".into()))))
                .await
                .is_err()
        );
        assert!(
            collect_pages(|_| std::future::ready(Ok(WatchHistoryPage::default())))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
