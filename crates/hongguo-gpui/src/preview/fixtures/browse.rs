use super::*;

/// 找剧页：筛选面板（体裁/主题/推荐 + 服务端八行里的代表性三行）。
pub(crate) fn browse_panel_rows() -> Vec<SelectorRow> {
    vec![
        SelectorRow {
            row_type: "genre".into(),
            row_name: "全部体裁".into(),
            items: vec![
                SelectorItem {
                    id: "short_play".into(),
                    name: "真人剧".into(),
                },
                SelectorItem {
                    id: "comic_series".into(),
                    name: "漫剧".into(),
                },
            ],
        },
        SelectorRow {
            row_type: "category_dim_theme".into(),
            row_name: "全部主题".into(),
            items: vec![
                SelectorItem {
                    id: "cate_262".into(),
                    name: "脑洞".into(),
                },
                SelectorItem {
                    id: "cate_265".into(),
                    name: "逆袭".into(),
                },
                SelectorItem {
                    id: "cate_268".into(),
                    name: "甜宠".into(),
                },
            ],
        },
        SelectorRow {
            row_type: "sort".into(),
            row_name: "全部推荐".into(),
            items: vec![
                SelectorItem {
                    id: "online_time".into(),
                    name: "最新上架".into(),
                },
                SelectorItem {
                    id: "hot_score".into(),
                    name: "最高热度".into(),
                },
            ],
        },
        SelectorRow {
            row_type: "gender".into(),
            row_name: "全部受众".into(),
            items: vec![
                SelectorItem {
                    id: "1".into(),
                    name: "男频".into(),
                },
                SelectorItem {
                    id: "0".into(),
                    name: "女频".into(),
                },
            ],
        },
    ]
}

/// 找剧页：一页浏览结果（封面 URL 用占位键，预览直接灌 covers）。
pub(crate) fn browse_feed_items() -> Vec<FeedItem> {
    SERIES
        .iter()
        .take(6)
        .enumerate()
        .map(|(i, (sid, title, heat))| FeedItem {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: format!("browse://cover-{i}"),
            horiz_cover: String::new(),
            vid: String::new(),
            episode_cnt: 24 + i as u32 * 6,
            play_cnt: 4_030_000 + i as i64 * 120_000,
            comment_count: 0,
            score: 8.0,
            tags: vec!["都市".into(), "逆袭".into()],
            season_tag: String::new(),
            heat_text: (*heat).into(),
            badge: String::new(),
            content_type: 1,
        })
        .collect()
}

/// 找剧页：搜索模式结果。
pub(crate) fn search_results() -> Vec<SearchResult> {
    SERIES
        .iter()
        .take(4)
        .enumerate()
        .map(|(i, (sid, title, _))| SearchResult {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: format!("browse://cover-{i}"),
            vid: String::new(),
            sub_title: "第1季·都市".into(),
            score: 8.0,
            play_cnt: 12_606_352 + i as i64 * 1_000_000,
            episode_cnt: 24 + i as u32 * 6,
            description: String::new(),
            tags: vec!["都市".into()],
            heat_text: String::new(),
        })
        .collect()
}

/// 找剧页：搜索联想（4 条，第 1、3 条带命中高亮切片——预览里看高亮片段
/// 与摘要行的排布；条目带 seriesId 时与纯词条目的渲染差异也在这里显形）。
pub(crate) fn suggest_items() -> Vec<SuggestItem> {
    SERIES
        .iter()
        .take(4)
        .enumerate()
        .map(|(i, (sid, title, _))| SuggestItem {
            word: (*title).into(),
            parts: if i % 2 == 0 {
                vec![
                    hongguo_api::search::SuggestPart {
                        text: title.chars().next().map(String::from).unwrap_or_default(),
                        hl: true,
                    },
                    hongguo_api::search::SuggestPart {
                        text: title.chars().skip(1).collect(),
                        hl: false,
                    },
                ]
            } else {
                Vec::new()
            },
            series_id: if i == 3 { String::new() } else { (*sid).into() },
            vid: String::new(),
            cover: format!("browse://cover-{i}"),
            abstract_text: format!("第{}季·都市·{}万热度", i + 1, 862 + i * 137),
            book_title: String::new(),
            book_cover: String::new(),
            book_score: String::new(),
        })
        .collect()
}
