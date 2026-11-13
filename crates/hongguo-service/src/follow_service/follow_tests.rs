use super::*;
use hongguo_api::interact::FollowUpdateItem;
use hongguo_common::model::settings::AccountState;

fn logged_in(state: &AppState) {
    let mut s = state.settings();
    s.account = Some(AccountState {
        mobile: "13800000000".into(),
        cookies: "sid=x".into(),
        user_name: String::new(),
        avatar_url: String::new(),
        user_id: "1".into(),
        login_at: 1,
        token: String::new(),
        raw_login: String::new(),
        raw_profile: String::new(),
    });
    state.replace_settings(s);
}

fn item(series_id: &str, episode_cnt: i64, title: &str) -> FollowUpdateItem {
    FollowUpdateItem {
        series_id: series_id.into(),
        title: title.into(),
        cover: String::new(),
        episode_cnt,
        latest_watched_vid_index: 0,
        finished: false,
        has_watched_latest_episode: false,
        follow_update_at: 0,
    }
}

#[test]
fn notices_empty_then_mark_and_clear() {
    let state = AppState::default();
    assert!(notices(&state).unwrap().is_empty());
    assert_eq!(mark_notices_read(&state, &[1, 2]).unwrap(), 0);
    clear_notices(&state).unwrap();
}

#[test]
fn reading_one_notice_preserves_the_other_unread_notice_until_clear() {
    let state = AppState::default();
    let first = state
        .store
        .follow_notice_insert(NOTICE_KIND_FOLLOW, "first", "First", "New episode", 1)
        .unwrap();
    let second = state
        .store
        .follow_notice_insert(NOTICE_KIND_FOLLOW, "second", "Second", "New episode", 2)
        .unwrap();
    assert_eq!(mark_notices_read(&state, &[first]).unwrap(), 1);
    let items = notices(&state).unwrap();
    assert_eq!(
        items
            .iter()
            .find(|item| item.id == first)
            .map(|item| item.read),
        Some(true)
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item.id == second)
            .map(|item| item.read),
        Some(false)
    );
    clear_notices(&state).unwrap();
    assert!(notices(&state).unwrap().is_empty());
}

#[tokio::test]
async fn aborting_the_owned_notice_task_releases_its_session_state() {
    let state = AppState::default();
    let weak = std::sync::Arc::downgrade(&state);
    let task = start_notice_task(state.clone());
    drop(state);
    assert!(weak.upgrade().is_some());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn check_updates_skips_when_logged_out() {
    let state = AppState::default();
    assert!(state.settings().account.is_none());
    assert_eq!(check_updates(&state).await.unwrap(), 0);
}

#[tokio::test]
async fn login_wall_blocks_toggle_list_is_following() {
    let state = AppState::default();
    let err = toggle(&state, "100", true).await.expect_err("未登录应挡");
    assert!(matches!(err, hongguo_common::AppError::Auth(_)));
    let err = list(&state, "").await.expect_err("未登录应挡");
    assert!(matches!(err, hongguo_common::AppError::Auth(_)));
    let err = is_following(&state, "100").await.expect_err("未登录应挡");
    assert!(matches!(err, hongguo_common::AppError::Auth(_)));
}

#[tokio::test]
async fn persist_new_notices_first_seen_only_sets_baseline() {
    let state = AppState::default();
    logged_in(&state);
    let n = persist_new_notices(&state, &[item("s1", 10, "剧A")])
        .await
        .unwrap();
    assert_eq!(n, 0, "首次见到只记基线不提示");
    assert_eq!(state.store.follow_baseline_get("s1").unwrap(), Some(10));
    assert!(notices(&state).unwrap().is_empty());
}

#[tokio::test]
async fn persist_new_notices_emits_when_episode_grows() {
    let state = AppState::default();
    logged_in(&state);
    state.store.follow_baseline_set("s1", 10).unwrap();
    let n = persist_new_notices(&state, &[item("s1", 12, "剧A")])
        .await
        .unwrap();
    assert_eq!(n, 1);
    let list = notices(&state).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].series_id, "s1");
    assert!(list[0].detail.contains("新增 2 集"));
    assert_eq!(state.store.follow_baseline_get("s1").unwrap(), Some(12));
}

#[tokio::test]
async fn persist_new_notices_skips_when_not_grown() {
    let state = AppState::default();
    logged_in(&state);
    state.store.follow_baseline_set("s1", 10).unwrap();
    let n = persist_new_notices(&state, &[item("s1", 10, "剧A")])
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert!(notices(&state).unwrap().is_empty());
}
