//! 播放地址解析：本地成品优先，其次在线流。

use super::{online, position};
use crate::app_state::AppState;
use hongguo_common::model::{PlayRequest, PlayResponse};
use hongguo_common::{AppError, AppResult};
use hongguo_store::Store;

/// 解析播放请求。`preferOnline` 只在**本地没有这一集**时才起作用：
/// 已下好的文件永远走 `hongguo-local://`，没必要重新下一遍。
pub async fn resolve_play(state: &AppState, request: &PlayRequest) -> AppResult<PlayResponse> {
    let resume_at = position::load(state, &request.series_id, request.vid_index)?;
    // 本地文件只有一版，谈不上切清晰度：菜单会拿到空列表而自动隐藏。
    // bytevc2 等需兼容转码时标 needs_compat，前端走同一 growing 管线（未登录本地实播最小接线）。

    // 调用方直接给了路径就用它
    if !request.file_path.trim().is_empty() {
        let url = hongguo_protocol::local::local_play_url(&request.file_path)
            .ok_or_else(|| AppError::InvalidArgs("文件路径无效".into()))?;
        return Ok(local_file_response(&request.file_path, url, resume_at));
    }

    // 已下载的那一集走本地协议
    let local = state
        .queue()
        .completed_path(&request.series_id, request.vid_index);

    if let Some(path) = local
        && let Some(url) = hongguo_protocol::local::local_play_url(&path)
    {
        return Ok(local_file_response(&path, url, resume_at));
    }

    if !request.prefer_online {
        return Err(AppError::NotFound(format!(
            "第 {} 集尚未下载",
            request.vid_index
        )));
    }

    // 兜底走在线流。vid 取自**剧集档案的分集表**（解析剧集时写入）——任务表只记下载进度，
    // 没下过的集在那里查不到 vid。档案里查不到就当场解析一次：播放页会同时发「拉分集」
    // 和「起播」两个请求，起播先到是常态，不补一次解析第一下点开必然报「缺少 vid」。
    let known = episode_vid(&state.store, &request.series_id, request.vid_index);
    let vid = match known {
        Some(vid) => vid,
        None => {
            log::info!(
                "[Play] {} 还没解析过，先解析出第 {} 集的 vid",
                request.series_id,
                request.vid_index
            );
            let env = state.api_env();
            let series =
                crate::series_service::resolver::resolve_series(&request.series_id, &env).await?;
            crate::series_service::registry::upsert_and_persist(state, series)?;
            episode_vid(&state.store, &request.series_id, request.vid_index).ok_or_else(|| {
                AppError::NotFound(format!("解析后仍找不到第 {} 集的 vid", request.vid_index))
            })?
        }
    };

    // 剧类型的持久来源：档案在库时整剧各集按 content_type 注册进取流注册表
    //（连播后续集靠它）。类型未知（旧档案 0）跳过——不猜值，取流走单集占位。
    if let Ok(Some(series)) = state.store.series_by_id(&request.series_id)
        && series.content_type != 0
    {
        for ep in &series.episodes {
            hongguo_api::play_url::register_content_type(&ep.vid, series.content_type);
        }
    }

    // 同一部剧的当前集和后面几集必须打在同一次批量里。分开打时服务端只给第一集，
    // 切到下一集就是空响应。
    warm_episode_window(state, &request.series_id, request.vid_index).await;

    // 进度事件的 key 用前端认的 `{seriesId}:{vidIndex}` 形态
    let progress_key = format!("{}:{}", request.series_id, request.vid_index);
    match online::prepare(state, &vid, &progress_key, request.definition).await {
        Ok(prepared) => {
            // 连播向后预取：流预取与无 HEVC 机器的预转共用 Settings.prefetch_episodes 档
            //（0=关闭）；幂等/去重/礼让在各 prefetch 内，越界集号由 episode_vid 兜住。
            crate::play_service::schedule_following_episodes(
                state,
                &request.series_id,
                request.vid_index,
            );
            let needs_compat = hongguo_bytevc2::is_bytevc2(&prepared.codec);
            Ok(PlayResponse {
                url: prepared.url,
                online: true,
                resume_at,
                error: String::new(),
                definition: prepared.definition,
                definitions: prepared.definitions,
                codec: prepared.codec,
                needs_compat,
            })
        }
        Err(e) => Ok(PlayResponse {
            url: String::new(),
            online: true,
            resume_at,
            error: e.to_string(),
            definition: 0,
            definitions: Vec::new(),
            codec: String::new(),
            needs_compat: false,
        }),
    }
}


/// 本地成品 / 显式 filePath：探测编码，bytevc2 标 needs_compat 以便正式播放器走 progressive 兼容。
fn local_file_response(path: &str, url: String, resume_at: f64) -> PlayResponse {
    let (codec, needs_compat) = probe_local_codec(path);
    PlayResponse {
        url,
        online: false,
        resume_at,
        error: String::new(),
        definition: 0,
        definitions: Vec::new(),
        codec,
        needs_compat,
    }
}

fn probe_local_codec(path: &str) -> (String, bool) {
    let Ok(demuxed) = hongguo_media::demux::demux_file(std::path::Path::new(path)) else {
        return (String::new(), false);
    };
    let Some(track) = demuxed.video_track() else {
        return (String::new(), false);
    };
    let codec = track.info.codec.clone();
    let needs_compat = hongguo_bytevc2::is_bytevc2(&codec);
    (codec, needs_compat)
}

/// 从剧集档案的分集表里取某一集的 vid。剧集档案是分集目录的唯一权威来源
///（`resolve_series` 拉到的分集含 vid 会写进数据库）；下载任务表只记进度，未下载的集没有记录。
/// 当前集连同后面若干集一次取回。`prefetch_episodes = 0` 时仍带上下一集，手动切集才有流。
async fn warm_episode_window(state: &AppState, series_id: &str, vid_index: u32) {
    let Some(series) = state.store.series_by_id(series_id).ok().flatten() else {
        return;
    };
    if series.content_type == 0 {
        return;
    }
    let last = vid_index.saturating_add(state.settings().prefetch_episodes.max(1));
    let group: Vec<String> = series
        .episodes
        .iter()
        .filter(|ep| {
            ep.vid_index >= vid_index
                && ep.vid_index <= last
                && !ep.vid.trim().is_empty()
                && !hongguo_api::play_url::has_play_cached(ep.vid.trim())
        })
        .map(|ep| ep.vid.trim().to_string())
        .collect();
    if group.is_empty() {
        return;
    }
    let mut map = std::collections::BTreeMap::new();
    map.insert(series.content_type.to_string(), group);
    crate::cold_start::wait().await;
    let env = state.api_env();
    match hongguo_api::play_url::fetch_play_batch(&map, crate::device::stream_cap(), &env).await {
        Ok(n) => log::info!("[Play] {series_id} 从第 {vid_index} 集批量取到 {n} 集"),
        Err(e) => log::warn!("[Play] 批量取流失败，回落单集: {e}"),
    }
}

pub(crate) fn episode_vid(store: &Store, series_id: &str, vid_index: u32) -> Option<String> {
    store
        .series_by_id(series_id)
        .ok()
        .flatten()?
        .episodes
        .iter()
        .find(|e| e.vid_index == vid_index)
        .map(|e| e.vid.clone())
        .filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_common::model::{Episode, Series};

    fn store_with(episodes: Vec<Episode>) -> Store {
        let store = Store::open_memory().expect("测试内存库");
        store
            .upsert_series(&Series {
                series_id: "7687919221593885758".into(),
                title: "二嫁有喜".into(),
                episodes,
                ..Default::default()
            })
            .expect("写入测试档案");
        store
    }

    #[test]
    fn finds_vid_for_undownloaded_episode() {
        let store = store_with(vec![
            Episode {
                vid_index: 1,
                vid: "vid-a".into(),
                ..Default::default()
            },
            Episode {
                vid_index: 2,
                vid: "vid-b".into(),
                ..Default::default()
            },
        ]);
        assert_eq!(
            episode_vid(&store, "7687919221593885758", 2).as_deref(),
            Some("vid-b")
        );
    }

    #[test]
    fn missing_index_is_none() {
        let store = store_with(vec![Episode {
            vid_index: 1,
            vid: "vid-a".into(),
            ..Default::default()
        }]);
        assert!(episode_vid(&store, "7687919221593885758", 9).is_none());
    }

    #[test]
    fn blank_vid_is_treated_as_missing() {
        let store = store_with(vec![Episode {
            vid_index: 1,
            vid: "   ".into(),
            ..Default::default()
        }]);
        assert!(episode_vid(&store, "7687919221593885758", 1).is_none());
    }

    #[test]
    fn unknown_series_is_none() {
        let store = store_with(vec![]);
        assert!(episode_vid(&store, "nope", 1).is_none());
    }

    #[test]
    fn local_bytevc2_file_marks_needs_compat() {
        let plain = std::path::PathBuf::from("/Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4");
        if !plain.is_file() {
            eprintln!("skip: sample missing");
            return;
        }
        let path = plain.to_string_lossy().to_string();
        let url = hongguo_protocol::local::local_play_url(&path).expect("url");
        let resp = super::local_file_response(&path, url, 0.0);
        assert!(resp.needs_compat, "bytevc2 本地文件必须 needs_compat");
        assert!(hongguo_bytevc2::is_bytevc2(&resp.codec), "codec={}", resp.codec);
        assert!(!resp.online);
        assert!(resp.error.is_empty());
    }

    #[test]
    fn local_missing_file_does_not_force_compat() {
        let resp = super::local_file_response(
            "/tmp/hongguo-definitely-missing-local-play.mp4",
            "hongguo-local://x".into(),
            1.5,
        );
        assert!(!resp.needs_compat);
        assert!(resp.codec.is_empty());
        assert_eq!(resp.resume_at, 1.5);
    }
}
