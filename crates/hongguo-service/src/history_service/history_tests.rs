use super::*;
use hongguo_common::model::LocalHistoryItem;
use hongguo_common::model::settings::AccountState;

fn sample(series_id: &str, position_ms: i64) -> LocalHistoryItem {
    LocalHistoryItem {
        series_id: series_id.into(),
        title: "t".into(),
        cover: String::new(),
        vid_index: 1,
        vid: "v1".into(),
        position_ms,
        duration_ms: 60_000,
        episode_cnt: 20,
        updated_at_ms: 1,
    }
}

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

#[tokio::test]
async fn record_offline_writes_local_and_leaves_pending() {
    let state = AppState::default();
    record(&state, &sample("s1", 5_000)).await.unwrap();
    let rows = state.store.local_history_list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].series_id, "s1");
    assert_eq!(rows[0].position_ms, 5_000);
    let pending = state.store.pending_local_history(10).unwrap();
    assert_eq!(pending.len(), 1, "未登录上报不走云端，应留待补传");
}

#[tokio::test]
async fn upload_pending_noop_when_logged_out() {
    let state = AppState::default();
    record(&state, &sample("s1", 1_000)).await.unwrap();
    upload_pending(&state).await;
    assert_eq!(state.store.pending_local_history(10).unwrap().len(), 1);
}

#[tokio::test]
async fn record_logged_in_keeps_local_row_even_if_cloud_fails() {
    // 无 mock 的云端：登录态会尝试上报并失败，本地行仍在且 reported=0。
    let state = AppState::default();
    logged_in(&state);
    record(&state, &sample("s2", 8_000)).await.unwrap();
    let rows = state.store.local_history_list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].series_id, "s2");
    assert_eq!(
        state.store.pending_local_history(10).unwrap().len(),
        1,
        "云端失败不得丢掉本地待补传标记"
    );
}
