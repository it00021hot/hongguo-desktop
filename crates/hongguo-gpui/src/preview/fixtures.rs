//! 预览假数据（`screenshot` feature）：合成封面与「字段齐平的 API 模型快照」。
//!
//! 全部离线、确定性（无网络、无时钟、无随机）：封面是按 seed 生成的渐变
//! 位图，模型快照的手填值对齐真实接口的形态（热度文案、档位、收藏数等），
//! 保证预览图和线上数据渲染走的是同一段布局代码。首字头像占位、进度条
//! 00:00 这类「引擎/网络缺席」的形态是预览的已知边界，见 `mod.rs` 头注释。

use std::sync::Arc;

use gpui_kit::RenderImage;
use hongguo_api::calendar::CalendarItem;
use hongguo_api::danmaku::{CommentItem, CreatorProfile, CreatorWork};
use hongguo_api::detail::RelatedItem;
use hongguo_api::discover::{FeedItem, SelectorItem, SelectorRow};
use hongguo_api::history::WatchHistoryItem;
use hongguo_api::interact::{BookshelfEntry, FollowUpdateItem, InteractionItem};
use hongguo_api::rank::RankItem;
use hongguo_api::search::{SearchResult, SuggestItem};
use hongguo_common::model::{
    DownloadTask, FollowNotice, MergeCandidate, MergeMode, MergePreflight, MergeStatus, MergeTask,
    NOTICE_KIND_FOLLOW, NOTICE_KIND_ONLINE, TaskStatus, VideoDefinition,
};
use hongguo_service::storage_service::usage::{StorageSeriesUsage, StorageUsage};

/// 磁盘清理页：总占用快照（3 部剧、7 个文件，合计约 2.4 GB）。
pub(crate) fn storage_usage() -> StorageUsage {
    let series = storage_series();
    StorageUsage {
        bytes: series.iter().map(|s| s.bytes).sum(),
        files: series.iter().map(|s| s.files).sum(),
    }
}

/// 磁盘清理页：按剧聚合的占用（只含磁盘上真有文件的剧）。
pub(crate) fn storage_series() -> Vec<StorageSeriesUsage> {
    vec![
        StorageSeriesUsage {
            series_id: SERIES[0].0.into(),
            title: SERIES[0].1.into(),
            bytes: 1_492_000_000,
            files: 3,
        },
        StorageSeriesUsage {
            series_id: SERIES[1].0.into(),
            title: SERIES[1].1.into(),
            bytes: 736_000_000,
            files: 2,
        },
        StorageSeriesUsage {
            series_id: SERIES[3].0.into(),
            title: SERIES[3].1.into(),
            bytes: 214_000_000,
            files: 2,
        },
    ]
}

/// 一键合并页：下拉候选（3 部剧，集数与体积对齐 storage 假数据量级）。
pub(crate) fn merge_candidates() -> Vec<MergeCandidate> {
    vec![
        MergeCandidate {
            series_id: SERIES[0].0.into(),
            series_title: SERIES[0].1.into(),
            episode_count: 5,
            total_size: 1_492_000_000,
        },
        MergeCandidate {
            series_id: SERIES[1].0.into(),
            series_title: SERIES[1].1.into(),
            episode_count: 3,
            total_size: 736_000_000,
        },
        MergeCandidate {
            series_id: SERIES[3].0.into(),
            series_title: SERIES[3].1.into(),
            episode_count: 2,
            total_size: 214_000_000,
        },
    ]
}

/// 一键合并页：一条合并中（42%）+ 一条已完成（有产物路径），id 固定
/// 保证预览确定性。
pub(crate) fn merge_tasks() -> Vec<MergeTask> {
    let mut running = MergeTask::new(
        SERIES[0].0,
        SERIES[0].1,
        &format!("{} 合集", SERIES[0].1),
        MergeMode::Compat,
    );
    running.id = "merge-running".into();
    running.status = MergeStatus::Running;
    running.episode_count = 5;
    running.percent = 42.0;
    let mut done = MergeTask::new(
        SERIES[1].0,
        SERIES[1].1,
        &format!("{} 合集", SERIES[1].1),
        MergeMode::Quick,
    );
    done.id = "merge-done".into();
    done.episode_count = 3;
    done.mark_completed("/预览目录/红果短剧/重生之千金归来 合集.mp4", 731_000_000);
    vec![running, done]
}

/// 一键合并页：预检通过（5 集 / 预计 1.4 GB / 磁盘富余，无 warning）。
pub(crate) fn merge_preflight() -> MergePreflight {
    MergePreflight {
        ok: true,
        episode_count: 5,
        estimated_size: 1_492_000_000,
        free_space: Some(128_000_000_000),
        codec_consistent: true,
        codec_mismatch_episode: None,
        warnings: Vec::new(),
    }
}

/// 点赞页：6 条已赞条目（剧名/计数取 SERIES 池，vid 定序保证预览确定）。
pub(crate) fn liked_items() -> Vec<InteractionItem> {
    SERIES
        .iter()
        .enumerate()
        .map(|(i, (sid, title, _))| InteractionItem {
            vid: format!("video{i}"),
            series_id: (*sid).into(),
            user_digg: true,
            digged_count: 5200 + i as i64 * 1379,
            followed: false,
            followed_cnt: 0,
            series_title: (*title).into(),
        })
        .collect()
}

/// 收藏页：6 条书架条目（content_type 交替真人/漫剧，封面时间戳递减）。
pub(crate) fn bookshelf_entries() -> Vec<BookshelfEntry> {
    SERIES
        .iter()
        .enumerate()
        .map(|(i, (sid, _, _))| BookshelfEntry {
            series_id: (*sid).into(),
            collect_time_ms: 1_760_000_000_000 - i as i64 * 86_400_000,
            content_type: if i % 2 == 0 { 1 } else { 1004 },
        })
        .collect()
}

/// 历史页：6 条观看记录（进度/时长/集数错开，时间基准贴近当前）。
pub(crate) fn watch_history_items() -> Vec<WatchHistoryItem> {
    const BASE_MS: i64 = 1_792_000_000_000; // 2026-10-13 前后
    SERIES
        .iter()
        .enumerate()
        .map(|(i, (sid, title, _))| WatchHistoryItem {
            series_id: (*sid).into(),
            title: (*title).into(),
            cover: String::new(), // 预览封面直接灌 covers，不走 URL
            vid_index: (3 + i) as i64,
            vid: format!("hv{i}"),
            position_ms: 300_000 + i as i64 * 60_000,
            duration_ms: 1_200_000,
            episode_cnt: 24 + i as i64 * 6,
            updated_at_ms: BASE_MS - i as i64 * 86_400_000,
        })
        .collect()
}

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

/// 剧目池：(series_id, 剧名, 热度文案)。在播剧取第一条，其余铺满三个
/// tab 源（沉浸流滚轮/切 tab 的数据面）。
pub(crate) const SERIES: [(&str, &str, &str); 6] = [
    ("7100001", "命运契约", "3182万热度"),
    ("7100002", "重生之千金归来", "2751万热度"),
    ("7100003", "闪婚老公是豪门", "2290万热度"),
    ("7100004", "我在长安当神探", "1876万热度"),
    ("7100005", "末日庇护所", "1544万热度"),
    ("7100006", "助理大人请自重", "1208万热度"),
];

/// 合成封面：seed 决定渐变双色，横向条纹位置随 seed 变化——肉眼可分辨
/// 「每行封面各不相同」即可，不追求内容语义。走 [`crate::image_upload::
/// render_image`]（crate 内唯一合法的 `RenderImage` 出口，BGRA 转换在那里）。
pub(crate) fn cover(seed: usize) -> Arc<RenderImage> {
    let (w, h) = (480u32, 270u32);
    // 双色从 seed 推导：低位置出基色，高位色相错开 97（素数，避免相邻
    // seed 撞相近色相）。
    let (r0, g0, b0) = pastel(seed);
    let (r1, g1, b1) = pastel(seed + 97);
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let t = (x + y) as f32 / (w + h) as f32;
        // 中段叠一条浅色横带（封面构图占位），其余线性渐变
        let band: f32 = if (y as f32 / h as f32 - 0.62).abs() < 0.05 {
            0.25
        } else {
            0.
        };
        let mix = |a: u8, b: u8| (a as f32 * (1. - t) + b as f32 * t + band * 64.) as u8;
        *px = image::Rgba([mix(r0, r1), mix(g0, g1), mix(b0, b1), 255]);
    }
    Arc::new(crate::image_upload::render_image(img))
}

/// seed → 柔和 RGB 三元组（低饱和高明度，浅深主题都不刺眼；47° 素数
/// 步进让相邻 seed 色相拉开，肉眼可分辨每张封面不同）。
fn pastel(seed: usize) -> (u8, u8, u8) {
    let hue = ((seed * 47) % 360) as f32;
    let (r, g, b) = hsv(hue, 0.35, 0.85);
    (r, g, b)
}

/// HSV → RGB（无外部依赖的最小实现，只服务假封面）。
fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.)) - 1.).abs();
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = v - c;
    (
        ((r + m) * 255.) as u8,
        ((g + m) * 255.) as u8,
        ((b + m) * 255.) as u8,
    )
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

/// 清晰度菜单快照（1080/720/540 三档，与真实档案下发同形态）。
pub(crate) fn definitions() -> Vec<VideoDefinition> {
    [(1080, 1920, 1080), (720, 1280, 720), (540, 960, 540)]
        .into_iter()
        .map(|(value, width, height)| VideoDefinition {
            value,
            width,
            height,
        })
        .collect()
}

/// 评论快照：覆盖首字头像占位（avatar 空）、点赞红心（user_digg）、
/// 剧评评分（score 非空）、多回复四类形态。
pub(crate) fn comments() -> Vec<CommentItem> {
    let base = 1_791_000_000_000_i64; // 展示用固定时刻（相对时间文案的输入）
    vec![
        CommentItem {
            comment_id: "c1".into(),
            user_id: "1001".into(),
            user_name: "追剧到三点的猫".into(),
            text: "这集反转太狠了，编剧是懂悬念的".into(),
            create_time: base,
            digg_count: 1024,
            reply_count: 36,
            user_digg: true,
            ..Default::default()
        },
        CommentItem {
            comment_id: "c2".into(),
            user_id: "1002".into(),
            user_name: "老张".into(),
            text: "男主这个发型真的很出戏，但是剧情可以".into(),
            create_time: base - 3_600_000,
            digg_count: 233,
            reply_count: 5,
            ..Default::default()
        },
        CommentItem {
            comment_id: "c3".into(),
            user_id: "1003".into(),
            user_name: "剧评人小王".into(),
            text: "节奏紧凑不注水，权当近年短剧标杆了。".into(),
            create_time: base - 86_400_000,
            digg_count: 87,
            score: "8.5".into(),
            score_suffix_text: "观看1小时后点评".into(),
            ..Default::default()
        },
        CommentItem {
            comment_id: "c4".into(),
            user_id: "1004".into(),
            user_name: "路过的甲乙丙".into(),
            text: "蹲一个第二季".into(),
            create_time: base - 172_800_000,
            digg_count: 12,
            reply_count: 2,
            ..Default::default()
        },
    ]
}

/// 下载任务快照：Running/Pending/Completed/Failed 四态各一（任务行的
/// 状态徽标、进度、操作按钮全被覆盖）。
pub(crate) fn download_tasks() -> Vec<DownloadTask> {
    let now = 1_791_000_000_i64;
    [
        (
            "t1",
            "命运契约",
            3,
            "第3集 惊天反转",
            TaskStatus::Running,
            486_000_000,
            1_073_741_824,
        ),
        (
            "t2",
            "命运契约",
            4,
            "第4集 真相浮出",
            TaskStatus::Pending,
            0,
            1_073_741_824,
        ),
        (
            "t3",
            "重生之千金归来",
            1,
            "第1集",
            TaskStatus::Completed,
            966_367_642,
            966_367_642,
        ),
        (
            "t4",
            "我在长安当神探",
            2,
            "第2集",
            TaskStatus::Failed,
            104_857_600,
            1_073_741_824,
        ),
    ]
    .into_iter()
    .map(
        |(id, title, vid_index, ep_title, status, downloaded, total)| DownloadTask {
            id: id.into(),
            series_id: "7100001".into(),
            series_title: title.into(),
            vid_index,
            vid: format!("vid-{vid_index}"),
            ep_title: ep_title.into(),
            // 完成态给成品路径（选集浮层的「已下载」标记按它判）
            file_path: if matches!(status, TaskStatus::Completed) {
                "/downloads/命运契约/第1集.mp4".into()
            } else {
                String::new()
            },
            temp_path: String::new(),
            status,
            downloaded,
            total,
            error: if matches!(status, TaskStatus::Failed) {
                "流地址过期（示例错误）".into()
            } else {
                String::new()
            },
            created_at: now,
            updated_at: now,
        },
    )
    .collect()
}

/// 系列剧季卡片快照：三季 + 一部未上线（「即将上线」预约分支）。
pub(crate) fn related_seasons() -> Vec<RelatedItem> {
    vec![
        RelatedItem {
            series_id: "7100001".into(),
            title: "命运契约 第一季".into(),
            tag: "第1季".into(),
            score: 8.2,
            play_cnt: 5_200_000,
            episode_cnt: 80,
            video_desc: "契约游戏的开始".into(),
            ..Default::default()
        },
        RelatedItem {
            series_id: "7100102".into(),
            title: "命运契约 第二季".into(),
            tag: "第2季".into(),
            score: 8.6,
            play_cnt: 3_100_000,
            episode_cnt: 65,
            video_desc: "终局之前的暗流".into(),
            ..Default::default()
        },
        RelatedItem {
            series_id: "7100103".into(),
            title: "命运契约 最终季".into(),
            tag: "第3季".into(),
            episode_cnt: 0,
            video_desc: "即将上线".into(),
            ..Default::default()
        },
    ]
}

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
