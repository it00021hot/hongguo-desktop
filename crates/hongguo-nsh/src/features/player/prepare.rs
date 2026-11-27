use crate::{bootstrap::Session, media::MediaLease};
use hongguo_common::{
    AppError, AppResult,
    model::{LocalHistoryItem, PlayRequest, PlayResponse, VideoDefinition},
};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Playback {
    /// Shared so silent handoff can keep the outgoing lease alive until the next first frame.
    pub(super) lease: Arc<MediaLease>,
    pub(super) resume_at: f64,
    pub(super) history: LocalHistoryItem,
    pub(super) next_episode: Option<u32>,
    pub(super) definitions: Vec<VideoDefinition>,
    pub(super) social: super::interaction::Target,
    pub(super) expected_duration: Option<f64>,
    pub(super) active_definition: u32,
    pub(super) automatic_definition: bool,
    pub(super) online: bool,
    pub(super) previous_episode: Option<u32>,
    pub(super) episode_indices: Vec<u32>,
}

pub(super) async fn prepare(
    session: Arc<Session>,
    id: String,
    episode: u32,
    definition: Option<u32>,
    reset_definition: bool,
    start_position: Option<f64>,
) -> AppResult<Playback> {
    let downloaded = session
        .state
        .queue()
        .completed_path(&id, episode)
        .is_some_and(|path| std::path::Path::new(&path).is_file());
    let response = hongguo_service::play_service::local::resolve_play(
        &session.state,
        &play_request(&id, episode, downloaded, definition, reset_definition),
    )
    .await?;
    check_response(&response)?;
    let series = session
        .state
        .store
        .series_by_id(&id)?
        .ok_or_else(|| AppError::NotFound(id.clone()))?;
    let current = series
        .episodes
        .iter()
        .find(|item| item.vid_index == episode)
        .ok_or_else(|| AppError::NotFound(episode.to_string()))?;
    let source = response.url;
    let lease = if let Some(path) = source.strip_prefix(&format!(
        "{}/f/",
        hongguo_common::media_url::scheme_base(hongguo_common::media_url::LOCAL_SCHEME)
    )) {
        let path = hongguo_common::media_url::resolve_path(path)
            .map_err(|error| AppError::Media(error.into()))?;
        session.media.playback_file(&path)
    } else if let Some(path) = source.strip_prefix(&hongguo_common::media_url::scheme_base(
        hongguo_common::media_url::STREAM_SCHEME,
    )) {
        let (definition, vid) = hongguo_common::media_url::parse_stream_path(path)
            .ok_or_else(|| AppError::Media("invalid stream source".into()))?;
        wait_for_stream(vid.to_owned(), definition).await?;
        session.media.online(vid.to_owned(), definition)
    } else {
        return Err(AppError::Media("unexpected playback source".into()));
    }
    .map_err(AppError::Media)?;
    let next_episode = series
        .episodes
        .iter()
        .filter(|item| item.vid_index > episode)
        .map(|item| item.vid_index)
        .min();
    let social = super::interaction::Target::from_episode(&series, current);
    let previous_episode = series
        .episodes
        .iter()
        .filter(|item| item.vid_index < episode)
        .map(|item| item.vid_index)
        .max();
    let episode_indices = series.episodes.iter().map(|item| item.vid_index).collect();
    let expected_duration = (current.duration > 0).then_some(current.duration as f64);
    let history = LocalHistoryItem {
        series_id: id,
        title: series.title,
        cover: series.cover,
        vid_index: episode,
        vid: current.vid.clone(),
        position_ms: 0,
        duration_ms: 0,
        episode_cnt: series.episode_count,
        updated_at_ms: 0,
    };
    let cloud_resume =
        session
            .personal
            .take_position(&session.state, &history.series_id, episode)?;
    let resume_at = start_position.unwrap_or(if response.resume_at > 0.0 {
        response.resume_at
    } else {
        cloud_resume
    });
    lease
        .prepare_codec_extension(resume_at)
        .map_err(AppError::Media)?;
    log::info!(
        "[Player] source series={} episode={} origin={} customRenderer={}",
        history.series_id,
        episode,
        if response.online { "online" } else { "local" },
        lease.frame_source().is_some()
    );
    Ok(Playback {
        lease: Arc::new(lease),
        resume_at,
        history,
        next_episode,
        definitions: response.definitions,
        social,
        expected_duration,
        active_definition: response.definition,
        automatic_definition: response.automatic_definition,
        online: response.online,
        previous_episode,
        episode_indices,
    })
}

fn check_response(response: &PlayResponse) -> AppResult<()> {
    if response.error.is_empty() {
        if response.direct {
            Ok(())
        } else {
            Err(AppError::CodecUnsupported(response.codec.clone()))
        }
    } else if response.error_kind == AppError::CodecUnsupported(String::new()).i18n_key() {
        Err(AppError::CodecUnsupported(response.error.clone()))
    } else {
        Err(AppError::Media(response.error.clone()))
    }
}

fn play_request(
    id: &str,
    episode: u32,
    downloaded: bool,
    definition: Option<u32>,
    reset: bool,
) -> PlayRequest {
    PlayRequest {
        series_id: id.into(),
        vid_index: episode,
        file_path: String::new(),
        prefer_online: reset || definition.is_some() || !downloaded,
        definition,
        reset_definition: reset,
    }
}

async fn wait_for_stream(vid: String, definition: u32) -> AppResult<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let guard = CancelOnDrop(Arc::new(AtomicBool::new(false)));
    let cancelled = guard.0.clone();
    // resolve_play starts filling in the background. A zero-byte read waits
    // for the real stream metadata without fetching or buffering the movie.
    hongguo_service::runtime::spawn_blocking(move || {
        hongguo_stream::read_range_cancellable(&vid, definition, 0, Some(0), move || {
            cancelled.load(Ordering::Acquire)
        })
        .map(|_| ())
        .map_err(|error| AppError::Media(error.to_string()))
    })
    .await
    .map_err(|error| AppError::Internal(format!("等待在线播放信息任务失败: {error}")))?
}
