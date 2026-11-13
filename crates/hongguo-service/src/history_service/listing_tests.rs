use super::*;

pub(super) fn item(series_id: &str, vid_index: i64, updated_at_ms: i64) -> WatchHistoryItem {
    WatchHistoryItem {
        series_id: series_id.into(),
        title: series_id.into(),
        cover: format!("{series_id}-cover"),
        vid_index,
        vid: format!("{series_id}-{vid_index}"),
        position_ms: 5_000,
        duration_ms: 120_000,
        episode_cnt: 504,
        updated_at_ms,
    }
}

#[test]
fn recent_last_episode_wins_over_delayed_cloud_row() {
    let cloud = item("7698639495289113662", 2, 1_794_576_900_000);
    let local = WatchHistoryItem {
        vid: "7702485557221280793".into(),
        position_ms: 113_986,
        duration_ms: 113_986,
        ..item(&cloud.series_id, 504, 1_794_577_057_294)
    };
    assert_eq!(merge(vec![cloud], vec![local.clone()]), vec![local]);
}

#[test]
fn newer_cloud_row_wins_as_a_whole() {
    let local = item("series", 504, 1_000);
    let cloud = WatchHistoryItem {
        vid: "cloud-second".into(),
        position_ms: 35_000,
        ..item("series", 2, 2_000)
    };
    assert_eq!(merge(vec![cloud.clone()], vec![local]), vec![cloud]);
}

#[test]
fn newer_rewatch_can_replace_a_completed_episode() {
    let cloud = item("series", 504, 1_000);
    let local = WatchHistoryItem {
        position_ms: 9_000,
        ..item("series", 1, 2_000)
    };
    assert_eq!(merge(vec![cloud], vec![local.clone()]), vec![local]);
}

#[test]
fn matching_timestamp_prefers_the_complete_local_row() {
    let cloud = item("series", 13, 2_000);
    let local = WatchHistoryItem {
        vid: "local-twelfth".into(),
        position_ms: 18_000,
        ..item("series", 12, 2_000)
    };
    assert_eq!(merge(vec![cloud], vec![local.clone()]), vec![local]);
}

#[test]
fn cloud_only_and_local_only_rows_are_sorted_by_recency() {
    let cloud = vec![item("cloud-old", 1, 1_000), item("cloud-new", 2, 3_000)];
    let local = vec![item("local-middle", 3, 2_000)];
    assert_eq!(
        merge(cloud, local)
            .into_iter()
            .map(|row| row.series_id)
            .collect::<Vec<_>>(),
        ["cloud-new", "local-middle", "cloud-old"]
    );
}

#[test]
fn repeated_rows_never_replace_a_newer_row_with_an_older_one() {
    let newest = item("series", 5, 5_000);
    let cloud = vec![
        item("series", 2, 2_000),
        newest.clone(),
        item("series", 3, 3_000),
    ];
    let local = vec![item("series", 4, 4_000), item("series", 1, 1_000)];
    assert_eq!(merge(cloud, local), vec![newest]);
}

#[test]
fn empty_and_single_source_lists_are_accepted() {
    let only = item("series", 1, 1_000);
    assert!(merge(Vec::new(), Vec::new()).is_empty());
    assert_eq!(merge(Vec::new(), vec![only.clone()]), vec![only.clone()]);
    assert_eq!(merge(vec![only.clone()], Vec::new()), vec![only]);
}

#[test]
fn known_episode_count_caps_only_the_read_result_index() {
    let cloud = WatchHistoryItem {
        episode_cnt: 128,
        ..item("cloud", 129, 1_000)
    };
    let local = item("local", 505, 2_000);
    let result = merge(vec![cloud.clone()], vec![local.clone()]);
    assert_eq!(
        result,
        vec![
            WatchHistoryItem {
                vid_index: 504,
                ..local.clone()
            },
            WatchHistoryItem {
                vid_index: 128,
                ..cloud.clone()
            },
        ]
    );
    assert_eq!(cloud.vid_index, 129);
    assert_eq!(local.vid_index, 505);
}

#[test]
fn unknown_episode_count_and_valid_indices_are_preserved() {
    let unknown = WatchHistoryItem {
        episode_cnt: 0,
        ..item("unknown", 129, 1_000)
    };
    let last = item("last", 504, 2_000);
    let middle = item("middle", 13, 3_000);
    assert_eq!(
        merge(vec![unknown.clone(), last.clone()], vec![middle.clone()]),
        vec![middle, last, unknown]
    );
}

#[test]
fn different_series_with_matching_timestamps_have_a_stable_order() {
    let rows = merge(
        vec![item("z", 1, 1_000), item("a", 1, 1_000)],
        vec![item("m", 1, 1_000)],
    );
    assert_eq!(
        rows.into_iter()
            .map(|row| row.series_id)
            .collect::<Vec<_>>(),
        ["a", "m", "z"]
    );
}
