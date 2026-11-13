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
        if let Some(response) = prepare_local_response(request, &path, resume_at)? {
            return Ok(response);
        }
    } else if !request.prefer_online {
        return Err(AppError::NotFound(format!(
            "第 {} 集尚未下载",
            request.vid_index
        )));
    }

    crate::cold_start::wait(state).await?;

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

fn prepare_local_response(
    request: &PlayRequest,
    path: &str,
    resume_at: f64,
) -> AppResult<Option<PlayResponse>> {
    let result = hongguo_protocol::local::local_play_url(path)
        .ok_or_else(|| AppError::InvalidArgs("文件路径无效".into()))
        .and_then(|url| local_file_response(path, url, resume_at));
    match result {
        Ok(response) => Ok(Some(response)),
        Err(error) if request.file_path.trim().is_empty() => {
            log::info!("[Play] 本地下载不可播放，重新获取在线原流: {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// 本地成品和显式 filePath 都探测编码，以便选中内置软解直播放。
fn local_file_response(path: &str, url: String, resume_at: f64) -> AppResult<PlayResponse> {
    let codec = local_playback_codec(path)?;
    Ok(PlayResponse {
        url,
        online: false,
        resume_at,
        error: String::new(),
        error_kind: String::new(),
        definition: 0,
        automatic_definition: false,
        definitions: Vec::new(),
        codec,
        direct: true,
    })
}

pub fn local_playback_codec(path: &str) -> AppResult<String> {
    let demuxed = hongguo_media::demux::demux_file(std::path::Path::new(path))?;
    let track = demuxed
        .video_track()
        .ok_or_else(|| AppError::Media("本地文件没有视频轨道".into()))?;
    let codec = track.info.codec.clone();
    let direct = crate::capability_service::plays_direct(&codec);
    if !direct {
        return Err(AppError::CodecUnsupported(codec));
    }
    Ok(codec)
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
    if let Err(error) = crate::cold_start::wait(state).await {
        log::warn!("[Play] 模型预取等待设备身份失败: {error}");
        return;
    }
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
    fn local_bytevc2_file_always_uses_builtin_playback() {
        use hongguo_video::fixtures::{TrackSpec, mp4};
        let dir = std::env::temp_dir().join(format!("hg-local-bytevc2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("bytevc2.mp4");
        std::fs::write(&plain, mp4(&[TrackSpec::video(1, b"bvc2", 64, 64)])).unwrap();
        let path = plain.to_string_lossy().to_string();
        let url = hongguo_protocol::local::local_play_url(&path).expect("url");
        let resp = super::local_file_response(&path, url, 0.0).unwrap();
        assert_eq!(resp.codec, "bvc2", "必须从合成文件真实探测出视频编码");
        assert!(resp.direct);
        assert!(!resp.online);
        assert!(resp.error.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_local_file_is_an_error_instead_of_native_fallback() {
        let resp = super::local_file_response(
            "/tmp/hongguo-definitely-missing-local-play.mp4",
            "hongguo-local://x".into(),
            1.5,
        );
        assert!(resp.is_err());
    }

    #[test]
    fn local_codec_routing_requires_builtin_decoding() {
        use hongguo_video::fixtures::{TrackSpec, mp4};
        let dir = std::env::temp_dir().join(format!("hg-local-routing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for codec in [b"hvc1", b"hev1", b"bvc2"] {
            let path = dir.join(format!("{}.mp4", String::from_utf8_lossy(codec)));
            std::fs::write(&path, mp4(&[TrackSpec::video(1, codec, 64, 64)])).unwrap();
            assert_eq!(
                super::local_playback_codec(path.to_str().unwrap()).unwrap(),
                String::from_utf8_lossy(codec)
            );
        }
        let path = dir.join("unsupported.mp4");
        std::fs::write(&path, mp4(&[TrackSpec::video(1, b"vp09", 64, 64)])).unwrap();
        assert!(matches!(
            super::local_playback_codec(path.to_str().unwrap()),
            Err(AppError::CodecUnsupported(_))
        ));
        std::fs::write(&path, b"not an MP4").unwrap();
        assert!(super::local_playback_codec(path.to_str().unwrap()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn local_h264_is_not_a_native_playback_exception() {
        use hongguo_video::fixtures::{TrackSpec, mp4};
        let dir =
            std::env::temp_dir().join(format!("hg-local-avc-rejected-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old-archive.mp4");
        std::fs::write(&path, mp4(&[TrackSpec::video(1, b"avc1", 64, 64)])).unwrap();
        assert!(matches!(
            super::local_playback_codec(path.to_str().unwrap()),
            Err(AppError::CodecUnsupported(_))
        ));
        let mut request = PlayRequest {
            series_id: "legacy".into(),
            vid_index: 1,
            file_path: String::new(),
            prefer_online: false,
            definition: None,
            reset_definition: false,
        };
        assert!(
            super::prepare_local_response(&request, path.to_str().unwrap(), 12.0)
                .unwrap()
                .is_none(),
            "旧下载必须重新取在线原流"
        );
        request.file_path = path.to_string_lossy().into_owned();
        assert!(matches!(
            super::prepare_local_response(&request, path.to_str().unwrap(), 12.0),
            Err(AppError::CodecUnsupported(_))
        ));
        request.file_path.clear();
        std::fs::write(&path, b"invalid old cache").unwrap();
        assert!(
            super::prepare_local_response(&request, path.to_str().unwrap(), 12.0)
                .unwrap()
                .is_none()
        );
        std::fs::remove_file(&path).unwrap();
        assert!(
            super::prepare_local_response(&request, path.to_str().unwrap(), 12.0)
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
