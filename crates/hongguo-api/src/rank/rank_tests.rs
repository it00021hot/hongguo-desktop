use super::*;

fn item(id: &str) -> RankItem {
    RankItem {
        series_id: id.into(),
        ..Default::default()
    }
}

/// 官方样本形状。
#[test]
fn parses_rank_items_from_cell_view() {
    let data: Value = serde_json::json!({
        "cell_view": {
            "cell_id": 7470092475068071998_i64,
            "cell_data": [
                { "video_data": [{
                    "series_id": "7681495327056071704",
                    "title": "万妖图录传第十二季",
                    "cover": "https://x.heic",
                    "vid": "7681497259883645977",
                    "sub_title": "玄幻·全200集",
                    "score": "8.0",
                    "play_cnt": 6197486,
                    "episode_cnt": 200,
                    "rec_text_item": { "RecommendText": "13707万最高热度" },
                    "secondary_info_list": [{ "content": "258.7万收藏" }],
                    "recommend_info": "{\"rank\":\"1\"}",
                    "video_desc": "以妖魔之血为墨",
                }]},
                { "video_data": [{
                    "series_id": "",
                    "title": "废条目"
                }]}
            ]
        }
    });
    let items = parse_rank_items(Some(&data)).unwrap();
    assert_eq!(items.len(), 1, "空 series_id 要跳过");
    let it = &items[0];
    assert_eq!(it.rank, 1);
    assert_eq!(it.rec_text, "13707万最高热度");
    assert_eq!(it.secondary_infos, vec!["258.7万收藏"]);
    assert_eq!(it.score, 8.0);
    assert_eq!(it.sub_title, "玄幻·全200集");
}

#[test]
fn parses_cell_selector_schema() {
    let data: Value = serde_json::json!({
        "cell_view": {
            "cell_selector": {
                "outer_row": {
                    "items": [
                        {
                            "show_name": "全部",
                            "selector_item_id": "all",
                            "sub_cell_selector": { "outer_row": { "items": [
                                {
                                    "show_name": "推荐榜",
                                    "selector_item_id": "ranklist_hot_sc",
                                    "sub_title": "10月4日已更新·基于红果观看/互动以及个人兴趣排序",
                                    "panel_selector": { "inner_rows": [
                                        {
                                            "row_name": "综合",
                                            "selection_type": 1,
                                            "items": [
                                                { "show_name": "总榜", "selector_item_id": "" },
                                                { "show_name": "女频", "selector_item_id": "gender_female", "is_selected": false },
                                                { "show_name": "男频", "selector_item_id": "gender_male" }
                                            ]
                                        },
                                        {
                                            "row_name": "时代背景",
                                            "items": [
                                                { "show_name": "古装", "selector_item_id": "cate_308" },
                                                { "show_name": "校园", "selector_item_id": "cate_4" }
                                            ]
                                        }
                                    ] }
                                },
                                { "show_name": "空面板子榜", "selector_item_id": "ranklist_subscribe" }
                            ] }}
                        },
                        {
                            "show_name": "漫剧",
                            "selector_item_id": "comic_series_rank",
                            "sub_cell_selector": { "outer_row": { "items": [
                                {
                                    "show_name": "推荐榜",
                                    "selector_item_id": "comic_series_hot_rank",
                                    "panel_selector": { "inner_rows": [
                                        { "row_name": "画风", "items": [
                                            { "show_name": "3d", "selector_item_id": "style_1685" }
                                        ] }
                                    ] }
                                }
                            ] }}
                        },
                        { "show_name": "无 id 项", "selector_item_id": "" }
                    ]
                }
            }
        }
    });
    let tabs = parse_cell_selector(Some(&data));
    assert_eq!(tabs.len(), 2, "空 id 的 tab 跳过");
    assert_eq!(tabs[0].id, "all");
    assert_eq!(tabs[0].name, "全部");
    assert_eq!(tabs[0].subs.len(), 2);
    let sub = &tabs[0].subs[0];
    assert_eq!(sub.id, "ranklist_hot_sc");
    assert_eq!(
        sub.description, "10月4日已更新·基于红果观看/互动以及个人兴趣排序",
        "子榜描述行（sub_title）供官方同款排版用"
    );
    assert_eq!(sub.panel.len(), 2);
    assert_eq!(sub.panel[0].name, "综合");
    assert_eq!(sub.panel[0].items[0].name, "总榜");
    assert_eq!(sub.panel[0].items[0].id, "");
    assert_eq!(sub.panel[0].items[1].id, "gender_female");
    assert_eq!(sub.panel[1].items[0].id, "cate_308");
    assert!(tabs[0].subs[1].panel.is_empty(), "无面板的子榜收空表");
    assert_eq!(tabs[1].subs[0].panel[0].items[0].id, "style_1685");
    assert!(parse_cell_selector(Some(&serde_json::json!({}))).is_empty());
}

/// 一级形态（老版本身份的退化结构）样本。
#[test]
fn parses_flat_login_selector_schema() {
    let data: Value = serde_json::json!({
        "cell_view": {
            "cell_selector": {
                "outer_row": {
                    "items": [
                        {
                            "show_name": "推荐榜",
                            "selector_item_id": "ranklist_hot_sc",
                            "sub_cell_selector": { "outer_row": { "items": [
                                { "show_name": "总榜", "selector_item_id": "" },
                                { "show_name": "女频", "selector_item_id": "gender_female" },
                                { "show_name": "男频", "selector_item_id": "gender_male" },
                                { "show_name": "古装", "selector_item_id": "cate_308" }
                            ] }}
                        },
                        {
                            "show_name": "热搜榜",
                            "selector_item_id": "ranklist_hot_search_sc",
                            "sub_cell_selector": null
                        }
                    ]
                }
            }
        }
    });
    let tabs = parse_cell_selector(Some(&data));
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].id, "ranklist_hot_sc");
    assert_eq!(tabs[0].name, "推荐榜");
    assert_eq!(tabs[0].subs.len(), 4, "空 id 的「总榜」要保留");
    assert_eq!(tabs[0].subs[0].id, "");
    assert_eq!(tabs[0].subs[0].name, "总榜");
    assert_eq!(tabs[0].subs[1].id, "gender_female");
    assert!(
        tabs[0].subs.iter().all(|s| s.panel.is_empty()),
        "一级形态无 panel 层"
    );
    assert!(tabs[1].subs.is_empty(), "无 sub_cell_selector 收空表");
}

#[test]
fn empty_id_kept_by_structure_not_by_label() {
    let data: Value = serde_json::json!({
        "cell_view": { "cell_selector": { "outer_row": { "items": [
            {
                "show_name": "推荐榜",
                "selector_item_id": "ranklist_hot_sc",
                "sub_cell_selector": { "outer_row": { "items": [
                    { "show_name": "全部", "selector_item_id": "" },
                    {
                        "show_name": "异常空 id 子榜",
                        "selector_item_id": "",
                        "panel_selector": { "inner_rows": [
                            { "row_name": "画风", "items": [
                                { "show_name": "3d", "selector_item_id": "style_1685" }
                            ] }
                        ] }
                    }
                ] }}
            }
        ] } } }
    });
    let tabs = parse_cell_selector(Some(&data));
    assert_eq!(tabs.len(), 1);
    assert_eq!(
        tabs[0].subs.len(),
        1,
        "空 id 叶子保留（文案是「全部」不是「总榜」）"
    );
    assert_eq!(tabs[0].subs[0].name, "全部");
}

#[test]
fn merge_into_book_dedups_keeping_order() {
    let mut book = RankSessionBook::default();
    merge_into_book(&mut book, vec![item("a"), item("b")]);
    merge_into_book(&mut book, vec![item("b"), item("c"), item("b")]);
    let ids: Vec<&str> = book.items.iter().map(|i| i.series_id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"], "重叠的 b 与页内重复的 b 都不重付");
}

#[test]
fn book_window_slices_and_terminates() {
    let mut book = RankSessionBook {
        items: vec![item("a"), item("b"), item("c")],
        upstream_has_more: true,
        ..Default::default()
    };
    let (items, has_more) = book_window(&book, 0);
    assert_eq!(
        items
            .iter()
            .map(|i| i.series_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(has_more, "上游未尽即有更多");
    let (items, has_more) = book_window(&book, 30);
    assert!(items.is_empty());
    assert!(has_more);
    book.upstream_has_more = false;
    let (items, has_more) = book_window(&book, 30);
    assert!(items.is_empty());
    assert!(!has_more, "上游已尽且账本浅于窗口=到底");
}

#[test]
fn selector_change_type_fresh_vs_continuation() {
    let fresh = RankReq {
        offset: 0,
        ..Default::default()
    };
    assert_eq!(selector_change_type(&fresh), "2");
    let page2 = RankReq {
        offset: 10,
        session_id: "s1",
        ..Default::default()
    };
    assert_eq!(selector_change_type(&page2), "1");
    let resume = RankReq {
        offset: 0,
        session_id: "s1",
        ..Default::default()
    };
    assert_eq!(selector_change_type(&resume), "1");
}

#[test]
fn rank_version_field_accepts_number_and_string() {
    let data: Value = serde_json::json!({ "rank_version": 1792166880_i64 });
    assert_eq!(
        rank_version_field(Some(&data)).as_deref(),
        Some("1792166880")
    );
    let data: Value = serde_json::json!({ "rank_version": "1792166880" });
    assert_eq!(
        rank_version_field(Some(&data)).as_deref(),
        Some("1792166880")
    );
    assert_eq!(rank_version_field(Some(&serde_json::json!({}))), None);
}

#[test]
fn book_keeps_delivery_order_for_filter_ids() {
    let mut book = RankSessionBook::default();
    merge_into_book(&mut book, vec![item("a"), item("b"), item("c")]);
    let fid = book
        .items
        .iter()
        .map(|i| i.series_id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(fid, "a,b,c");
}

#[test]
fn parses_background_selector_schema() {
    let data: Value = serde_json::json!({
        "cell_view": { "cell_selector": { "outer_row": { "items": [
            {
                "show_name": "全部",
                "selector_item_id": "all",
                "sub_cell_selector": { "outer_row": { "items": [
                    {
                        "show_name": "必看榜",
                        "selector_item_id": "ranklist_must_watch",
                        "background_selector": { "inner_rows": [
                            { "row_name": "综合", "items": [
                                { "show_name": "总榜", "selector_item_id": "date_all" }
                            ] },
                            { "row_name": "2026年", "items": [
                                { "show_name": "10月", "selector_item_id": "date_202610", "is_selected": true },
                                { "show_name": "9月", "selector_item_id": "date_202609" }
                            ] }
                        ] }
                    }
                ] }}
            }
        ] } } }
    });
    let tabs = parse_cell_selector(Some(&data));
    let bg = &tabs[0].subs[0].background;
    assert_eq!(bg.len(), 2);
    assert_eq!(bg[0].name, "综合");
    assert_eq!(bg[0].items[0].id, "date_all");
    assert_eq!(bg[1].name, "2026年");
    assert_eq!(bg[1].items[1].id, "date_202609");
}

#[test]
fn rank_items_keep_delivery_order() {
    let data: Value = serde_json::json!({
        "cell_data": [
            { "video_data": [
                { "series_id": "b", "recommend_info": "{\"rank\":\"2\"}" },
                { "series_id": "a", "recommend_info": "{\"rank\":\"1\"}" },
                { "series_id": "c" }
            ]}
        ]
    });
    let items = parse_rank_items(Some(&data)).unwrap();
    let ids: Vec<&str> = items.iter().map(|i| i.series_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["b", "a", "c"],
        "交付序不动，rank 只当有无名次的标记"
    );
}

#[test]
fn collect_walks_nested_cells() {
    let v: Value =
        serde_json::json!({ "a": { "video_data": [1] }, "b": [ { "video_data": [2] } ] });
    let mut out = Vec::new();
    collect_video_data(&v, &mut out);
    assert_eq!(out.len(), 2);
}
