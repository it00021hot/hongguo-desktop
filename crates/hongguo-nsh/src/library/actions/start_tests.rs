use super::*;
use hongguo_common::model::{AccountState, Episode, PlaybackPosition, Series};

const SERIES: &str = "9007199254740993";

async fn playback_session(
    local_at: i64,
    cloud_at: i64,
) -> std::sync::Arc<crate::bootstrap::Session> {
    let state = AppState::default();
    let mut settings = state.settings();
    settings.account = Some(AccountState {
        user_id: "fixture".into(),
        login_at: 1,
        mobile: String::new(),
        cookies: String::new(),
        user_name: String::new(),
        avatar_url: String::new(),
        token: String::new(),
        raw_login: String::new(),
        raw_profile: String::new(),
    });
    state.replace_settings(settings);
    let archive = Series {
        series_id: SERIES.into(),
        episodes: vec![
            Episode {
                vid: "first".into(),
                vid_index: 3,
                ..Default::default()
            },
            Episode {
                vid: "latest".into(),
                vid_index: 9,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    state.store.upsert_series(&archive).unwrap();
    state
        .store
        .save_playback_position(
            SERIES,
            3,
            &PlaybackPosition {
                current_time: 12.0,
                duration: 100.0,
                updated_at: local_at,
            },
        )
        .unwrap();
    let cloud = hongguo_api::history::WatchHistoryItem {
        series_id: SERIES.into(),
        vid_index: 9,
        position_ms: 31_000,
        duration_ms: 100_000,
        updated_at_ms: cloud_at,
        ..Default::default()
    };
    crate::bootstrap::Session::for_test(state, vec![cloud])
        .await
        .unwrap()
}

#[tokio::test]
async fn personal_card_uses_the_unified_cloud_start_and_one_shot_hint() {
    let session = playback_session(1000, 2000).await;
    let entry = Entry {
        series_id: SERIES.into(),
        liked_vid: Some("first".into()),
        ..Default::default()
    };
    assert_eq!(
        start_playback(session.clone(), entry).await.unwrap(),
        (SERIES.into(), 9)
    );
    assert_eq!(
        session
            .personal
            .take_position(&session.state, SERIES, 9)
            .unwrap(),
        31.0
    );
    assert_eq!(
        session
            .personal
            .take_position(&session.state, SERIES, 9)
            .unwrap(),
        0.0
    );
}

#[tokio::test]
async fn a_newer_local_start_wins_and_a_deleted_liked_video_does_not_block_the_series() {
    let session = playback_session(3000, 2000).await;
    let entry = Entry {
        series_id: SERIES.into(),
        liked_vid: Some("deleted".into()),
        ..Default::default()
    };
    assert_eq!(start_playback(session.clone(), entry).await.unwrap().1, 3);
    assert_eq!(
        session
            .personal
            .take_position(&session.state, SERIES, 3)
            .unwrap(),
        12.0
    );
}
