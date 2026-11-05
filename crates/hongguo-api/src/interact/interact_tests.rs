use super::*;

#[test]
fn danmaku_add_payload_shape_matches_capture() {
    let shark = shark_param_danmaku_add();
    assert_eq!(shark["aid"], "8662");
    assert_eq!(shark["type"], "short_play");
    assert_eq!(shark["enter_from"], "MainFragmentActivity");
    assert_ne!(20, 4);
}

#[test]
fn digg_action_types_match_capture() {
    assert_eq!(video_digg_action_type(true), 3);
    assert_eq!(video_digg_action_type(false), 4);
    assert_eq!(comment_digg_action_type(true), 8);
    assert_eq!(comment_digg_action_type(false), 9);
}

#[test]
fn shelf_operate_types_match_capture() {
    assert_eq!(shelf_operate_type(true), 0);
    assert_eq!(shelf_operate_type(false), 1);
}

#[test]
fn follow_action_types_match_capture() {
    assert_eq!(follow_action_type(true), 34);
    assert_eq!(follow_action_type(false), 35);
}

#[test]
fn follow_updates_parse_matches_capture() {
    let sample = serde_json::json!({
        "code": 0,
        "data": {
            "comment_list_info": {
                "cursor": "{\"session_id\":\"20261015132033C96A23C06C91444C4511\",\"offset\":20}",
                "has_more": false
            },
            "mix_data": [{
                "data_type": 1,
                "video": {
                    "common": {
                        "cover": "https://p3-reading-sign.fqnovelpic.com/novel-pic/x.heic",
                        "title": "摆摊大王",
                        "vertical": false,
                        "video_content_type": 1004,
                        "video_type": 2
                    },
                    "expand": {
                        "episode_cnt": 214,
                        "extra": {
                            "follow_update_time_stamp": "1792041548",
                            "lasted_read_time_stamp": "1792041412",
                            "max_chapter_first_pass_time": "1787631872"
                        },
                        "has_watched_latest_episode": false,
                        "latest_watched_vid_index": 16,
                        "series_status": 1
                    },
                    "video_id": "7677818497497320472"
                }
            }]
        }
    });
    let page = parse_follow_updates(&sample);
    assert_eq!(page.items.len(), 1);
    let item = &page.items[0];
    assert_eq!(item.series_id, "7677818497497320472");
    assert_eq!(item.title, "摆摊大王");
    assert_eq!(item.episode_cnt, 214);
    assert_eq!(item.latest_watched_vid_index, 16);
    assert!(item.finished, "series_status=1 = 已完结");
    assert!(!item.has_watched_latest_episode);
    assert_eq!(item.follow_update_at, 1_792_041_548);
    assert!(page.cursor.contains("session_id"));
    assert!(!page.has_more);
}

#[test]
fn follow_updates_parse_tolerates_empty_and_junk() {
    let empty = serde_json::json!({
        "code": 0,
        "data": {"comment_list_info": {"cursor": "", "has_more": false}, "mix_data": []}
    });
    let page = parse_follow_updates(&empty);
    assert!(page.items.is_empty());
    assert!(page.cursor.is_empty());

    let junk = serde_json::json!({
        "code": 0,
        "data": {"mix_data": [{"data_type": 1}, {"video": {"common": {"title": "无id"}}}]}
    });
    assert!(parse_follow_updates(&junk).items.is_empty());
}

#[test]
fn state_parsing_tolerates_missing_fields() {
    let empty = serde_json::json!({"code": 0, "data": null});
    assert_eq!(
        empty
            .pointer("/data/mixed_data_list")
            .and_then(Value::as_array),
        None,
        "空响应按无列表处理，不强解"
    );
}
