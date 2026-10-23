use super::*;

/// 预约页：一页预约条目（已上线带评分/播放数；待上线带上线时间）。
pub(crate) fn reservation_items(online: bool) -> Vec<CalendarItem> {
    const BASE_MS: i64 = 1_792_100_000_000;
    SERIES
        .iter()
        .take(4)
        .enumerate()
        .map(|(i, (sid, title, heat))| CalendarItem {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: String::new(), // 预览封面直接灌 covers
            vid: String::new(),
            score: if online { 8.6 - i as f64 * 0.3 } else { 0.0 },
            play_cnt: if online {
                4_030_000 + i as i64 * 120_000
            } else {
                0
            },
            episode_cnt: if online { 24 + i as u32 * 6 } else { 0 },
            description: "战乱年代，一介孤女在渡口撑起商行，卷入朝堂风云。".into(),
            category: "古装".into(),
            rec_tags: vec![(*heat).into(), "权谋".into(), "大女主".into()],
            publish_time: if online {
                BASE_MS - (i as i64 + 1) * 86_400_000
            } else {
                BASE_MS + (i as i64 + 1) * 86_400_000 * 3
            },
            is_online: online,
            has_subscribed: true,
        })
        .collect()
}

/// 追更页：6 条追更条目（追平/未看/已完结交错，集数错开）。
pub(crate) fn follow_items() -> Vec<FollowUpdateItem> {
    SERIES
        .iter()
        .enumerate()
        .map(|(i, (sid, title, _))| FollowUpdateItem {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: String::new(), // 预览封面直接灌 covers
            episode_cnt: 24 + i as i64 * 12,
            latest_watched_vid_index: match i % 3 {
                0 => 0,
                1 => 10,
                _ => (24 + i as i64 * 12).min(30),
            },
            finished: i == 2 || i == 5,
            has_watched_latest_episode: i == 2 || i == 5,
            follow_update_at: 1_792_100_000 - i as i64 * 86_400,
        })
        .collect()
}

/// 追更页「更新提醒」：4 条通知（追更更新 / 预约上线交错，两条未读）。
pub(crate) fn follow_notices() -> Vec<FollowNotice> {
    vec![
        FollowNotice {
            id: 4,
            kind: NOTICE_KIND_FOLLOW.into(),
            series_id: SERIES[0].0.into(),
            title: SERIES[0].1.into(),
            detail: "追的剧更新了，新增 2 集（共 216 集）".into(),
            at: 1_792_041_548,
            read: false,
        },
        FollowNotice {
            id: 3,
            kind: NOTICE_KIND_ONLINE.into(),
            series_id: SERIES[3].0.into(),
            title: SERIES[3].1.into(),
            detail: "预约的《我在长安当神探》已上线，去看看".into(),
            at: 1_791_955_148,
            read: false,
        },
        FollowNotice {
            id: 2,
            kind: NOTICE_KIND_FOLLOW.into(),
            series_id: SERIES[1].0.into(),
            title: SERIES[1].1.into(),
            detail: "追的剧更新了，新增 4 集（共 88 集）".into(),
            at: 1_791_782_000,
            read: true,
        },
        FollowNotice {
            id: 1,
            kind: NOTICE_KIND_FOLLOW.into(),
            series_id: SERIES[4].0.into(),
            title: SERIES[4].1.into(),
            detail: "追的剧更新了，新增 1 集（共 60 集）".into(),
            at: 1_791_609_200,
            read: true,
        },
    ]
}

/// 创作者页：资料头（头像由种子的合成封面充当）。
pub(crate) fn creator_profile() -> CreatorProfile {
    CreatorProfile {
        user_id: "2007062623837002".into(),
        user_name: "林间剧场".into(),
        avatar: String::new(), // 预览头像直接灌 avatar 位图
        description: "专注都市逆袭题材的短剧出品方，代表作《命运契约》《重生之千金归来》。".into(),
    }
}

/// 创作者页：作品（6 部，剧名/热度取 SERIES 池，集数与播放量错开）。
pub(crate) fn creator_works() -> Vec<CreatorWork> {
    SERIES
        .iter()
        .enumerate()
        .map(|(i, (sid, title, _))| CreatorWork {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: String::new(), // 预览封面直接灌 covers
            intro: String::new(),
            episode_cnt: 24 + i as i64 * 12,
            play_cnt: 3_120_000 + i as i64 * 456_000,
            hot: 31_820_000 - i as i64 * 3_000_000,
            tags: Vec::new(),
        })
        .collect()
}

/// 新剧推荐：6 部新剧（封面 URL 用占位键，预览直接灌 covers）。
pub(crate) fn new_drama_items() -> Vec<RankItem> {
    SERIES
        .iter()
        .take(6)
        .enumerate()
        .map(|(i, (sid, title, _))| RankItem {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: format!("new://cover-{i}"),
            vid: String::new(),
            rank: (i + 1) as u32,
            sub_title: "都市·全80集".into(),
            score: 8.6 - i as f64 * 0.2,
            play_cnt: 0,
            episode_cnt: 80 - i as u32 * 6,
            rec_text: format!("{} 上新", 120 + i * 13),
            secondary_infos: Vec::new(),
            description: "战乱年代，一介孤女在渡口撑起商行。".into(),
            tags: vec!["都市".into(), "逆袭".into()],
            upcoming: i == 5,
            reserved: false,
            season: String::new(),
        })
        .collect()
}

/// 上新日历：当日条目（publish_time 落在 2026-10-14 本地，日期条筛得上）。
pub(crate) fn calendar_items(count: usize) -> Vec<CalendarItem> {
    // 2026-10-14 12:00 (+08:00) 的 UTC 毫秒
    const BASE_MS: i64 = 1_791_950_400_000;
    SERIES
        .iter()
        .take(count)
        .enumerate()
        .map(|(i, (sid, title, _))| CalendarItem {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: format!("new-cal://cover-{i}"),
            vid: String::new(),
            score: 8.6 - i as f64 * 0.3,
            play_cnt: 0,
            episode_cnt: if i % 2 == 0 { 80 } else { 0 },
            description: "战乱年代，一介孤女在渡口撑起商行，卷入朝堂风云。".into(),
            category: "古装".into(),
            rec_tags: vec![format!("{} 人预约", 12 + i * 7), "权谋".into()],
            publish_time: BASE_MS + i as i64 * 3_600_000,
            is_online: i % 2 == 0,
            has_subscribed: false,
        })
        .collect()
}

/// 角标/季标签池（与 [`SERIES`] 同下标配对：有新剧/高分/今日更新三类
/// 角标，季标签集数各不相同，验收叠层排版）。
pub(crate) fn stream_tags(index: usize) -> (&'static str, &'static str) {
    const POOLS: [(&str, &str); SERIES.len()] = [
        ("", "全80集"),
        ("新剧上线", "全65集"),
        ("", "全52集"),
        ("高分", "全40集"),
        ("", "全36集"),
        ("今日更新", "全48集"),
    ];
    POOLS[index % POOLS.len()]
}
