//! 播放页数据编排：推荐流、取流、分集档案、简介、弹幕与互动回显。
//!
//! 全部是 `background()`（tokio）上的 async 函数，结果由调用方（`mod.rs`
//! 的组件任务）写回信号。**壳不复制业务**：接口与 service 逐个对应
//! （与 Tauri 壳 `src/service/commands` 的同一批底层调用）。

use hongguo_api::danmaku::{CommentPage, Danmaku, ReplyPage};
use hongguo_api::detail::{EpisodeList, SeriesMeta};
use hongguo_api::discover::FeedPage;
use hongguo_api::interact::{FollowUpdatePage, InteractionState};
use hongguo_common::model::VideoDefinition;
use hongguo_service::app_state::AppState;

/// 等渐进流注册的上限（对齐 serve 自身的 30s 预算）。
const REGISTER_TIMEOUT_MS: u64 = 30_000;
/// 注册轮询间隔。
const REGISTER_POLL_MS: u64 = 20;

/// prepare 一集并等渐进流注册（注册完才有可读数据，提前给 URL 会 404）。
/// 返回 (实际档位, 本集全部档位)。
pub(crate) async fn prepare_url(
    state: &AppState,
    vid: &str,
    definition: Option<u32>,
) -> Result<(u32, Vec<VideoDefinition>), String> {
    let prepared = hongguo_service::play_service::online::prepare(
        state,
        vid,
        crate::pump::PROGRESS_KEY,
        definition,
    )
    .await
    .map_err(|e| e.to_string())?;
    let picked = prepared.definition;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(REGISTER_TIMEOUT_MS);
    while hongguo_stream::total_len(vid, picked).is_none() {
        if std::time::Instant::now() >= deadline {
            // 超时不当作失败：serve 自己还有 30s 等待，播放器继续请求即可。
            log::warn!("[Player] {vid}@{picked} 注册等待超时，交给 serve 兜底");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(REGISTER_POLL_MS)).await;
    }
    Ok((picked, prepared.definitions))
}

/// 推荐流一页（`tab_type` / 会话游标 / offset，与 Tauri 首页同参）。
pub(crate) async fn fetch_feed(
    tab: &str,
    session: &str,
    offset: i64,
    state: &AppState,
) -> Result<FeedPage, String> {
    hongguo_api::recommend::fetch_recommend_feed(tab, session, offset, &[], &state.api_env())
        .await
        .map_err(|e| e.to_string())
}

/// 分集档案（选集 / 自动连播 / 公开计数的数据源）。
pub(crate) async fn fetch_series(series_id: &str, state: &AppState) -> Result<EpisodeList, String> {
    hongguo_api::detail::fetch_episode_list(series_id, &state.api_env())
        .await
        .map_err(|e| e.to_string())
}

/// 详情元信息（简介 / 连载状态，`video_detail` 接口）。
pub(crate) async fn fetch_meta(series_id: &str, state: &AppState) -> Result<SeriesMeta, String> {
    hongguo_api::detail::fetch_series_meta(series_id, &state.api_env())
        .await
        .map_err(|e| e.to_string())
}

/// 一集的全部弹幕（后端已按时间轴排序；服务端异常时整条线失败，
/// 播放不受影响——调用方按「引导条 + 日志」降级）。
pub(crate) async fn fetch_danmaku(
    vid: &str,
    series_id: &str,
    state: &AppState,
) -> Result<Vec<Danmaku>, String> {
    hongguo_api::danmaku::fetch_danmaku_all(vid, series_id, &state.api_env())
        .await
        .map_err(|e| e.to_string())
}

/// 评论区一页（单集维度流：group=vid / group_type=30，与 Tauri 面板同源）。
pub(crate) async fn fetch_comments(
    vid: &str,
    series_id: &str,
    cursor: &str,
    state: &AppState,
) -> Result<CommentPage, String> {
    hongguo_api::danmaku::fetch_comments_page(vid, series_id, cursor, &state.api_env())
        .await
        .map_err(|e| e.to_string())
}

/// 一条评论的回复页。
pub(crate) async fn fetch_replies(
    vid: &str,
    series_id: &str,
    comment_id: &str,
    cursor: &str,
    state: &AppState,
) -> Result<ReplyPage, String> {
    hongguo_api::danmaku::fetch_comment_replies(
        vid,
        series_id,
        comment_id,
        cursor,
        &state.api_env(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// 互动回显（最近互动列表；匿名调用被服务端静默拒，调用方只在登录后拉）。
pub(crate) async fn interaction_state(state: &AppState) -> Result<InteractionState, String> {
    hongguo_api::interact::fetch_interaction_state(&state.api_env())
        .await
        .map_err(|e| e.to_string())
}

/// 追更列表首页（追更钮显隐的状态源之一；meta 未到时回落判定）。
pub(crate) async fn follow_list(state: &AppState) -> Result<FollowUpdatePage, String> {
    hongguo_api::interact::fetch_follow_updates("", &state.api_env())
        .await
        .map_err(|e| e.to_string())
}
