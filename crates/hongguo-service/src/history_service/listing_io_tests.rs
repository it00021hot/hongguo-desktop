use super::*;
use hongguo_common::{
    AppError,
    model::{LocalHistoryItem, settings::AccountState},
};
use std::{
    cell::{Cell, RefCell},
    future::ready,
};

fn local(series_id: &str, index: u32, updated_at_ms: i64) -> LocalHistoryItem {
    LocalHistoryItem {
        series_id: series_id.into(),
        title: format!("local-{series_id}"),
        cover: "cover".into(),
        vid_index: index,
        vid: format!("{series_id}-{index}"),
        position_ms: 9_000,
        duration_ms: 60_000,
        episode_cnt: 504,
        updated_at_ms,
    }
}

fn sign_in(state: &AppState, user_id: &str, login_at: i64) {
    let mut settings = state.settings();
    settings.account = Some(AccountState {
        mobile: String::new(),
        cookies: String::new(),
        user_name: String::new(),
        avatar_url: String::new(),
        user_id: user_id.into(),
        login_at,
        token: String::new(),
        raw_login: String::new(),
        raw_profile: String::new(),
    });
    state.replace_settings(settings);
}

#[tokio::test]
async fn anonymous_list_reads_memory_store_and_caps_without_writing() {
    let state = AppState::default();
    state
        .store
        .upsert_local_history(&local("9007199254740993", 505, 2_000))
        .unwrap();
    let page = list(&state, None).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].series_id, "9007199254740993");
    assert_eq!(page.items[0].vid_index, 504);
    assert_eq!(page.items[0].vid, "9007199254740993-505");
    assert_eq!((page.has_more, page.next_offset, page.total), (false, 0, 1));
    assert_eq!(state.store.local_history_list().unwrap()[0].vid_index, 505);
}

#[tokio::test]
async fn list_reads_local_after_all_cloud_pages_and_returns_complete_metadata() {
    let state = AppState::default();
    sign_in(&state, "account", 1);
    let calls = RefCell::new(Vec::new());
    let page = list_with(&state, account(&state), |offset| {
        calls.borrow_mut().push(offset);
        if offset == 1 {
            state
                .store
                .upsert_local_history(&local("series", 504, 3_000))
                .unwrap();
        }
        ready(Ok(WatchHistoryPage {
            items: vec![super::tests::item("series", 2, 1_000 + offset)],
            has_more: offset == 0,
            next_offset: 1,
            total: 2,
        }))
    })
    .await
    .unwrap();
    assert_eq!(*calls.borrow(), [0, 1]);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].vid_index, 504);
    assert_eq!(page.items[0].title, "local-series");
    assert_eq!((page.has_more, page.next_offset, page.total), (false, 0, 1));
}

#[tokio::test]
async fn all_pages_accept_empty_intermediate_pages_and_keep_latest_duplicate() {
    let calls = RefCell::new(Vec::new());
    let rows = collect_pages(|offset| {
        calls.borrow_mut().push(offset);
        ready(Ok(WatchHistoryPage {
            items: if offset == 1 {
                Vec::new()
            } else {
                vec![super::tests::item("9007199254740993", offset + 1, offset)]
            },
            has_more: offset < 2,
            next_offset: offset + 1,
            total: 1,
        }))
    })
    .await
    .unwrap();
    assert_eq!(*calls.borrow(), [0, 1, 2]);
    let merged = merge(rows, Vec::new());
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].series_id, "9007199254740993");
    assert_eq!(merged[0].vid_index, 3);
}

#[tokio::test]
async fn stalled_cursor_and_page_failure_never_return_partial_success() {
    assert!(
        collect_pages(|_| ready(Ok(WatchHistoryPage {
            has_more: true,
            ..Default::default()
        })))
        .await
        .is_err()
    );
    assert!(
        collect_pages(|offset| ready(if offset == 0 {
            Ok(WatchHistoryPage {
                items: vec![super::tests::item("series", 1, 1)],
                has_more: true,
                next_offset: 1,
                total: 1,
            })
        } else {
            Err(AppError::Network("mock offline".into()))
        }))
        .await
        .is_err()
    );
    assert!(
        collect_pages(|_| ready(Ok(WatchHistoryPage::default())))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn account_switch_and_same_user_relogin_reject_list_results() {
    for (next_user, next_login) in [("other", 1), ("account", 2)] {
        let state = AppState::default();
        sign_in(&state, "account", 1);
        let result = list_with(&state, account(&state), |_| {
            sign_in(&state, next_user, next_login);
            ready(Ok(WatchHistoryPage::default()))
        })
        .await;
        assert!(matches!(result, Err(AppError::Auth(_))));
    }
}

#[tokio::test]
async fn successful_cloud_delete_removes_local_and_pending_rows() {
    let state = AppState::default();
    sign_in(&state, "account", 1);
    state
        .store
        .upsert_local_history(&local("series", 504, 1_000))
        .unwrap();
    let entries = vec![("series".into(), "series-504".into(), 504)];
    delete_with(&state, account(&state), &entries, || {
        assert_eq!(state.store.local_history_list().unwrap().len(), 1);
        ready(Ok(()))
    })
    .await
    .unwrap();
    assert!(state.store.local_history_list().unwrap().is_empty());
    assert!(state.store.pending_local_history(1).unwrap().is_empty());
}

#[tokio::test]
async fn cloud_delete_failure_preserves_local_row_for_retry() {
    let state = AppState::default();
    sign_in(&state, "account", 1);
    state
        .store
        .upsert_local_history(&local("series", 504, 1_000))
        .unwrap();
    let entries = vec![("series".into(), "series-504".into(), 504)];
    let result = delete_with(&state, account(&state), &entries, || {
        ready(Err(AppError::Network("mock offline".into())))
    })
    .await;
    assert!(matches!(result, Err(AppError::Network(_))));
    assert_eq!(state.store.local_history_list().unwrap().len(), 1);
    assert_eq!(state.store.pending_local_history(1).unwrap().len(), 1);
}

#[tokio::test]
async fn anonymous_delete_skips_cloud_and_removes_only_selected_series() {
    let state = AppState::default();
    state
        .store
        .upsert_local_history(&local("remove", 1, 1_000))
        .unwrap();
    state
        .store
        .upsert_local_history(&local("keep", 2, 2_000))
        .unwrap();
    let entries = vec![("remove".into(), "remove-1".into(), 1)];
    delete_with(&state, account(&state), &entries, || {
        ready(Err(AppError::Internal(
            "anonymous delete called cloud".into(),
        )))
    })
    .await
    .unwrap();
    let rows = state.store.local_history_list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].series_id, "keep");
}

#[tokio::test]
async fn account_change_during_cloud_delete_preserves_local_row() {
    let state = AppState::default();
    sign_in(&state, "account", 1);
    state
        .store
        .upsert_local_history(&local("series", 504, 1_000))
        .unwrap();
    let entries = vec![("series".into(), "series-504".into(), 504)];
    let result = delete_with(&state, account(&state), &entries, || {
        sign_in(&state, "account", 2);
        ready(Ok(()))
    })
    .await;
    assert!(matches!(result, Err(AppError::Auth(_))));
    assert_eq!(state.store.local_history_list().unwrap().len(), 1);
}

fn stale_accounts() -> [Option<HistoryAccount>; 3] {
    [
        Some(HistoryAccount {
            user_id: "previous".into(),
            login_at: 1,
        }),
        Some(HistoryAccount {
            user_id: "current".into(),
            login_at: 1,
        }),
        None,
    ]
}

#[tokio::test]
async fn stale_expected_accounts_never_fetch_rows() {
    for expected in stale_accounts() {
        let state = AppState::default();
        sign_in(&state, "current", 2);
        let calls = Cell::new(0);
        let result = list_with(&state, expected.clone(), |_| {
            calls.set(calls.get() + 1);
            ready(Ok(WatchHistoryPage::default()))
        })
        .await;
        assert!(matches!(result, Err(AppError::Auth(_))));
        assert_eq!(calls.get(), 0);
        assert!(matches!(
            list(&state, expected).await,
            Err(AppError::Auth(_))
        ));
    }
}

#[tokio::test]
async fn stale_expected_accounts_never_delete_rows() {
    for expected in stale_accounts() {
        let state = AppState::default();
        sign_in(&state, "current", 2);
        state
            .store
            .upsert_local_history(&local("series", 504, 1_000))
            .unwrap();
        let entries = vec![("series".into(), "series-504".into(), 504)];
        let calls = Cell::new(0);
        let result = delete_with(&state, expected.clone(), &entries, || {
            calls.set(calls.get() + 1);
            ready(Ok(()))
        })
        .await;
        assert!(matches!(result, Err(AppError::Auth(_))));
        assert_eq!(calls.get(), 0);
        assert_eq!(state.store.local_history_list().unwrap().len(), 1);
        assert!(matches!(
            delete(&state, &[], expected).await,
            Err(AppError::Auth(_))
        ));
    }
}
