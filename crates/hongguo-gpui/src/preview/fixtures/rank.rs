use super::*;

/// 排行榜快照：前三甲 + 常规行 + 未上线行（预约按钮分支）。
/// 排行榜：内容 tab × 子榜（两级形态，含子榜自带筛选面板）。
pub(crate) fn rank_tabs() -> Vec<hongguo_api::rank::RankTab> {
    use hongguo_api::rank::{RankPanelItem, RankPanelRow, RankSubList, RankTab};
    let panel = vec![RankPanelRow {
        name: "分类".into(),
        items: vec![
            RankPanelItem {
                id: String::new(),
                name: "总榜".into(),
            },
            RankPanelItem {
                id: "cate_262".into(),
                name: "脑洞".into(),
            },
            RankPanelItem {
                id: "cate_265".into(),
                name: "逆袭".into(),
            },
        ],
    }];
    let subs = |names: [(&str, &str, &str); 3]| -> Vec<RankSubList> {
        names
            .into_iter()
            .map(|(id, name, desc)| RankSubList {
                id: id.into(),
                name: name.into(),
                description: desc.into(),
                panel: if id == "ranklist_hot_sc" {
                    panel.clone()
                } else {
                    Vec::new()
                },
                // 月榜历史行（b588e92 起 schema 必填；假数据不带）
                background: Vec::new(),
            })
            .collect()
    };
    vec![
        RankTab {
            id: "all".into(),
            name: "全部".into(),
            subs: subs([
                ("ranklist_hot_sc", "热榜", "10月14日更新"),
                ("ranklist_new_sc", "新剧榜", "10月14日更新"),
                ("ranklist_reserve", "预约榜", "按预约人数"),
            ]),
        },
        RankTab {
            id: "real".into(),
            name: "真人剧".into(),
            subs: subs([
                ("ranklist_real_hot", "真人热榜", "10月14日更新"),
                ("ranklist_real_new", "真人新剧", "10月14日更新"),
                ("ranklist_real_reserve", "真人预约", "按预约人数"),
            ]),
        },
        RankTab {
            id: "comic".into(),
            name: "漫剧".into(),
            subs: subs([
                ("ranklist_comic_hot", "漫剧热榜", "10月14日更新"),
                ("ranklist_comic_new", "漫剧新剧", "10月14日更新"),
                ("ranklist_comic_reserve", "漫剧预约", "按预约人数"),
            ]),
        },
    ]
}

pub(crate) fn rank_items() -> Vec<RankItem> {
    let rows: [(&str, &str, &str, f64, i64, u32, bool); 6] = [
        (
            "7100001",
            "命运契约",
            "都市·全80集",
            9.1,
            137_070_000,
            80,
            false,
        ),
        (
            "7100002",
            "重生之千金归来",
            "逆袭·全65集",
            8.7,
            98_450_000,
            65,
            false,
        ),
        (
            "7100003",
            "闪婚老公是豪门",
            "甜宠·全52集",
            8.3,
            76_210_000,
            52,
            false,
        ),
        (
            "7100004",
            "我在长安当神探",
            "古装·全40集",
            8.0,
            51_006_000,
            40,
            false,
        ),
        (
            "7100005",
            "末日庇护所",
            "科幻·全36集",
            7.8,
            33_890_000,
            36,
            false,
        ),
        (
            "7100006",
            "助理大人请自重",
            "职场·全48集",
            7.5,
            21_450_000,
            48,
            true,
        ),
    ];
    rows.into_iter()
        .enumerate()
        .map(
            |(i, (series_id, title, sub_title, score, play_cnt, episode_cnt, upcoming))| RankItem {
                series_id: series_id.into(),
                title: title.into(),
                rank: (i + 1) as u32,
                sub_title: sub_title.into(),
                score,
                play_cnt,
                episode_cnt,
                rec_text: format!("{}万最高热度", play_cnt / 10_000),
                secondary_infos: vec![format!("{}万收藏", play_cnt / 100_000)],
                upcoming,
                ..Default::default()
            },
        )
        .collect()
}
