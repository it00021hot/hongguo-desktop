use super::*;

#[test]
fn parses_recommend_feed_entries() {
    let data: Value = serde_json::json!({
        "has_more": true,
        "next_offset": 6,
        "session_id": "20261008181149086C4BCA8151179AD830",
        "cell_view": {
            "cell_data": [
                { "video_data": [{
                    "series_id": 7677187660368055321_i64,
                    "vid": 7677203040796953624_i64,
                    "rec_tags": [
                        { "content": "真人剧新番榜 No.4", "data_type": 1 },
                        { "content": "4920万热度", "data_type": 1 },
                        { "content": "不展示的杂项", "data_type": 9 }
                    ],
                    "video_detail": {
                        "series_id_str": "7677187660368055321",
                        "series_title": "谁把我仙侠游戏退出键扣了3D版",
                        "series_cover": "",
                        "series_cover_uri": "https://p3-sign.douyinpic.com/x~tplv.image",
                        "episode_cnt": 248,
                        "series_play_cnt": 17050000,
                        "content_type": 1004,
                        "category_schema": "[{\"category_id\":755,\"name\":\"脑洞\"},{\"category_id\":37,\"name\":\"穿越\"}]",
                        "tag_info": { "text": "新剧", "bg_color": ["#00AE83"] },
                        "secondary_infos": [
                            { "content": "第1季", "highlight": true },
                            { "content": "9.6分", "data_type": 4 },
                            { "content": "异界", "data_type": 3 }
                        ]
                    }
                }]},
                { "video_data": [{ "vid": 123_i64, "video_detail": { "series_title": "缺 id 的废条目" } }] }
            ]
        }
    });
    let page = parse_recommend_feed(Some(&data)).unwrap();
    assert_eq!(page.items.len(), 1, "缺 series_id 的条目要跳过");
    let it = &page.items[0];
    assert_eq!(
        it.series_id, "7677187660368055321",
        "顶层数字 series_id 无损转字符串"
    );
    assert_eq!(it.title, "谁把我仙侠游戏退出键扣了3D版");
    assert_eq!(it.vid, "7677203040796953624");
    assert!(it.cover.contains("douyinpic"));
    assert_eq!(it.episode_cnt, 248);
    assert_eq!(it.play_cnt, 17_050_000);
    assert_eq!(it.content_type, 1004);
    assert_eq!(it.tags, vec!["脑洞", "穿越"]);
    assert_eq!(
        it.season_tag, "第1季",
        "data_type 缺省的 secondary_info 是季徽"
    );
    assert_eq!(
        it.heat_text, "真人剧新番榜 No.4 · 4920万热度",
        "热度行 = rec_tags data_type=1 拼接"
    );
    assert_eq!(it.badge, "新剧", "运营角标 = video_detail.tag_info.text");
    assert_eq!(it.score, 9.6, "评分 = secondary data_type=4 去掉「分」");
    assert!(page.has_more);
    assert_eq!(page.next_offset, 6);
    assert_eq!(page.session_id, "20261008181149086C4BCA8151179AD830");
}

#[test]
fn parses_feed_tab_bar() {
    let value = serde_json::json!({
        "data": { "tab_item": [
            { "tab_type": 45, "title": "关注" },
            { "tab_type": 36, "title": "漫剧", "bookstore_id": null },
            { "tab_type": 39, "title": "真人剧" },
            { "title": "缺 tab_type 的废条目" },
            { "tab_type": "16", "title": "推荐", "bookstore_id": "7294257258253254693" }
        ]}
    });
    let tabs = parse_feed_tabs(&value).unwrap();
    assert_eq!(
        tabs.iter().map(|t| t.tab_type.as_str()).collect::<Vec<_>>(),
        ["45", "36", "39", "16"],
        "保序且跳过残缺条目"
    );
    assert_eq!(tabs[0].title, "关注");
    assert_eq!(tabs[0].bookstore_id, None, "游客态关注无数据面");
    assert_eq!(tabs[1].bookstore_id, None);
    assert_eq!(tabs[3].title, "推荐");
    assert_eq!(tabs[3].bookstore_id.as_deref(), Some("7294257258253254693"));
}
