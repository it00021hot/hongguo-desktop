//! 播放地址解析：显式文件、本地播放与在线清晰度选择。

use super::{online, position};
use crate::app_state::AppState;
use hongguo_api::params::PlayerModelScene;
use hongguo_common::model::{PlayRequest, PlayResponse};
use hongguo_common::{AppError, AppResult};
use hongguo_store::Store;

/// 显式文件路径优先；`preferOnline` 起播返回在线流及可选档位。
pub async fn resolve_play(state: &AppState, request: &PlayRequest) -> AppResult<PlayResponse> {
    let resume_at = position::load(state, &request.series_id, request.vid_index)?;
    if let Some(path) = local_playback_path(state, request) {
        let url = hongguo_protocol::local::local_play_url(&path)
            .ok_or_else(|| AppError::InvalidArgs("文件路径无效".into()))?;
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

    warm_episode_window(state, &request.series_id, request.vid_index).await;

    // 进度事件的 key 用前端认的 `{seriesId}:{vidIndex}` 形态
    let progress_key = format!("{}:{}", request.series_id, request.vid_index);
    super::resolution::remember(
        state,
        &request.series_id,
        request.definition,
        request.reset_definition,
    )
    .await?;
    match online::prepare(state, &vid, &progress_key, request.definition).await {
        Ok(prepared) => {
            crate::play_service::schedule_following_episodes(
                state,
                &request.series_id,
                request.vid_index,
            );
            let direct = crate::capability_service::plays_direct(&prepared.codec);
            Ok(PlayResponse {
                url: prepared.url,
                online: true,
                resume_at,
                error: String::new(),
                error_kind: String::new(),
                definition: prepared.definition,
                automatic_definition: prepared.automatic_definition,
                definitions: prepared.definitions,
                codec: prepared.codec,
                direct,
            })
        }
        Err(e) => Ok(PlayResponse {
            url: String::new(),
            online: true,
            resume_at,
            error: e.to_string(),
            error_kind: e.i18n_key().to_owned(),
            definition: 0,
            automatic_definition: false,
            definitions: Vec::new(),
            codec: String::new(),
            direct: false,
        }),
    }
}

fn local_playback_path(state: &AppState, request: &PlayRequest) -> Option<String> {
    if !request.file_path.trim().is_empty() {
        return Some(request.file_path.clone());
    }
    if request.prefer_online {
        return None;
    }
    state
        .queue()
        .completed_path(&request.series_id, request.vid_index)
}

/// 本地成品和显式 filePath 都探测编码，以便选中内置软解直播放。
fn local_file_response(path: &str, url: String, resume_at: f64) -> PlayResponse {
    let (codec, direct) = probe_local_codec(path);
    PlayResponse {
        url,
        online: false,
        resume_at,
        error: String::new(),
        error_kind: String::new(),
        definition: 0,
        automatic_definition: false,
        definitions: Vec::new(),
        codec,
        direct,
    }
}

/// 本地成品文件是否交给壳的直播放（按文件里视频轨的实际编码判断，读不出编码为假）。
pub fn plays_direct_file(path: &str) -> bool {
    probe_local_codec(path).1
}

fn probe_local_codec(path: &str) -> (String, bool) {
    let Ok(demuxed) = hongguo_media::demux::demux_file(std::path::Path::new(path)) else {
        return (String::new(), false);
    };
    let Some(track) = demuxed.video_track() else {
        return (String::new(), false);
    };
    let codec = track.info.codec.clone();
    let direct = crate::capability_service::plays_direct(&codec);
    (codec, direct)
}

/// 首集预热只取该集模型。
pub async fn warm_episode_window(state: &AppState, series_id: &str, vid_index: u32) {
    if vid_index == 1 {
        prefetch_episode_models(state, series_id, 1..=1, PlayerModelScene::Preload).await;
    }
}

pub(super) async fn prefetch_episode_models(
    state: &AppState,
    series_id: &str,
    episodes: std::ops::RangeInclusive<u32>,
    scene: PlayerModelScene,
) {
    let store = state.store.clone();
    let id = series_id.to_owned();
    let Ok(Ok(Some(series))) =
        crate::runtime::spawn_blocking(move || store.series_by_id(&id)).await
    else {
        return;
    };
    if series.content_type == 0 {
        return;
    }
    let vids: Vec<_> = series
        .episodes
        .into_iter()
        .filter(|ep| episodes.contains(&ep.vid_index))
        .map(|ep| ep.vid)
        .collect();
    let map = std::collections::BTreeMap::from([(series.content_type.to_string(), vids)]);
    crate::cold_start::wait().await;
    let env = state.api_env();
    if let Err(error) =
        hongguo_api::play_url::fetch_play_scene(&map, scene, crate::device::stream_cap(), &env)
            .await
    {
        log::warn!("[Play] {scene:?} 模型预取失败: {error}");
    }
}

/// 未下载集的 vid 也来自剧集档案。
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

    #[test]
    fn online_playback_keeps_quality_selection_when_the_episode_is_downloaded() {
        let state = std::sync::Arc::new(crate::app_state::AppStateInner::default());
        let mut task = hongguo_common::model::DownloadTask::new("series", "剧集", 1, "vid", "");
        task.mark_completed("/downloads/episode.mp4", 1);
        state.queue().enqueue(task);
        let mut request = PlayRequest {
            series_id: "series".into(),
            vid_index: 1,
            file_path: String::new(),
            prefer_online: true,
            definition: None,
            reset_definition: false,
        };

        assert_eq!(local_playback_path(&state, &request), None);
        request.definition = Some(720);
        assert_eq!(local_playback_path(&state, &request), None);
        request.prefer_online = false;
        request.definition = None;
        assert_eq!(
            local_playback_path(&state, &request).as_deref(),
            Some("/downloads/episode.mp4")
        );
        request.prefer_online = true;
        request.file_path = "/chosen/episode.mp4".into();
        assert_eq!(
            local_playback_path(&state, &request).as_deref(),
            Some("/chosen/episode.mp4")
        );
        request.file_path.clear();
        request.prefer_online = false;
        request.vid_index = 2;
        assert_eq!(local_playback_path(&state, &request), None);
    }

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
    fn local_bytevc2_file_marks_direct() {
        let plain = std::path::PathBuf::from(
            "/Users/milo/qa-hongguo-offline-data/inputs/hg-bytevc2-plain.mp4",
        );
        if !plain.is_file() {
            eprintln!("skip: sample missing");
            return;
        }
        let path = plain.to_string_lossy().to_string();
        let url = hongguo_protocol::local::local_play_url(&path).expect("url");
        let resp = super::local_file_response(&path, url, 0.0);
        assert!(resp.direct, "bytevc2 本地文件必须 direct");
        assert!(
            hongguo_bytevc2::is_bytevc2(&resp.codec),
            "codec={}",
            resp.codec
        );
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
        assert!(!resp.direct);
        assert!(resp.codec.is_empty());
        assert_eq!(resp.resume_at, 1.5);
    }
}
