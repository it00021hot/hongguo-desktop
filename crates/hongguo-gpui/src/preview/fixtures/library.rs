use super::*;

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
