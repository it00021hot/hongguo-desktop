//! 播放地址解析：本地成品优先，其次在线流。

use tauri::State;

use super::{online, position};
use crate::app_state::AppState;
use crate::domain::model::{PlayRequest, PlayResponse};
use crate::error::{AppError, AppResult};
use crate::store::Store;

/// 解析播放请求。
///
/// `preferOnline` 只在**本地没有这一集**时才起作用：已经下好的文件永远走
/// `hongguo-local://`，没必要重新下一遍。
pub async fn resolve_play(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    request: &PlayRequest,
) -> AppResult<PlayResponse> {
    let resume_at = position::load(state, &request.series_id, request.vid_index);
    // 本地文件只有一版，谈不上切清晰度：菜单会拿到空列表而自动隐藏
    let local_response = |url: String, online: bool| PlayResponse {
        url,
        online,
        resume_at,
        error: String::new(),
        definition: 0,
        definitions: Vec::new(),
    };

    // 调用方直接给了路径就用它
    if !request.file_path.trim().is_empty() {
        let url = crate::protocol::local::local_play_url(&request.file_path)
            .ok_or_else(|| AppError::InvalidArgs("文件路径无效".into()))?;
        return Ok(local_response(url, false));
    }

    // 已下载的那一集走本地协议
    let local = state
        .queue()
        .completed_path(&request.series_id, request.vid_index);

    if let Some(path) = local {
        if let Some(url) = crate::protocol::local::local_play_url(&path) {
            return Ok(local_response(url, false));
        }
    }

    if !request.prefer_online {
        return Err(AppError::NotFound(format!(
            "第 {} 集尚未下载",
            request.vid_index
        )));
    }

    // 兜底走在线流。vid 取自**剧集档案的分集表**（解析剧集时写入），
    // 不能查下载任务表：任务表只记下载进度，没下过的剧集在那里查不到 vid，
    // 结果就是「点开没下过的集永远播不了」。
    //
    // 档案里查不到就当场解析一次。播放页会同时发「拉分集」和「起播」两个请求，
    // 起播先到是常态；不在这儿补一次解析，第一下点开必然报「第 N 集缺少 vid」。
    // （Store 是 DB 线程门面，查询本身阻塞完成，没有「锁跨 await」问题。）
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
            let series = crate::service::series_service::resolver::resolve_series(
                &request.series_id,
                &env,
            )
            .await?;
            crate::service::series_service::registry::upsert_and_persist(state, series)?;
            episode_vid(&state.store, &request.series_id, request.vid_index).ok_or_else(
                || AppError::NotFound(format!("解析后仍找不到第 {} 集的 vid", request.vid_index)),
            )?
        }
    };

    // 进度事件的 key 用前端认的 `{seriesId}:{vidIndex}` 形态
    let progress_key = format!("{}:{}", request.series_id, request.vid_index);
    match online::prepare(app, &vid, &progress_key, request.definition, state.settings(), &state.api_env()).await {
        Ok(prepared) => Ok(PlayResponse {
            url: prepared.url,
            online: true,
            resume_at,
            error: String::new(),
            definition: prepared.definition,
            definitions: prepared.definitions,
        }),
        Err(e) => Ok(PlayResponse {
            url: String::new(),
            online: true,
            resume_at,
            error: e.to_string(),
            definition: 0,
            definitions: Vec::new(),
        }),
    }
}

/// 从剧集档案的分集表里取某一集的 vid。
///
/// 剧集档案是分集目录的唯一权威来源：`resolve_series` 拉到的分集（含 vid）
/// 会写进数据库。下载任务表只记进度，未下载的集在那里根本没有记录。
fn episode_vid(store: &Store, series_id: &str, vid_index: u32) -> Option<String> {
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
    use crate::domain::model::{Episode, Series};

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
        // 这一集从没下过，下载任务表里必然没有记录——正是原来的 bug 场景
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
        // 分集在但 vid 是空串（接口没给）时不能拿空串去取流
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
}
