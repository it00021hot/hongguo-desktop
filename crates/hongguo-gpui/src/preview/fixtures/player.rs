use super::*;

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
