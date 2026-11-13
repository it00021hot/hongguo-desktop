use super::model::{Action, Snapshot, Target, Toggle};
use hongguo_api::interact;
use hongguo_common::{AppError, AppResult};
use hongguo_service::AppState;

const SERIES_ONGOING: i64 = 0;

pub(super) async fn load(state: AppState, target: Target, logged_in: bool) -> AppResult<Snapshot> {
    let mut snapshot = Snapshot {
        like: Toggle::new(false, target.likes),
        collect: Toggle::new(false, target.collections),
        ..Default::default()
    };
    if !logged_in {
        return Ok(snapshot);
    }
    let account = state
        .settings()
        .account
        .ok_or_else(|| AppError::Auth("未登录".into()))?;
    let env = state.api_env();
    let (interactions, shelf, follows, meta) = tokio::try_join!(
        interact::fetch_interaction_state(&env),
        interact::fetch_bookshelf(&account.user_id, &env),
        hongguo_service::follow_service::list(&state, ""),
        hongguo_api::detail::fetch_series_meta(&target.series_id, &env),
    )?;
    let video = interactions
        .items
        .iter()
        .find(|item| item.vid == target.vid);
    let series = interactions
        .items
        .iter()
        .find(|item| item.series_id == target.series_id);
    snapshot.like = Toggle::new(
        video.is_some_and(|item| item.user_digg),
        if target.likes > 0 {
            target.likes
        } else {
            video.map_or(0, |item| item.digged_count)
        },
    );
    snapshot.collect = Toggle::new(
        shelf.iter().any(|item| item.series_id == target.series_id),
        if target.collections > 0 {
            target.collections
        } else {
            series.map_or(0, |item| item.followed_cnt)
        },
    );
    snapshot.follow = Toggle::new(
        follows
            .items
            .iter()
            .any(|item| item.series_id == target.series_id),
        0,
    );
    snapshot.show_follow = meta.series_status == SERIES_ONGOING;
    Ok(snapshot)
}

pub(super) async fn submit(
    state: AppState,
    target: Target,
    action: Action,
    selected: bool,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("未登录".into()));
    }
    if action == Action::Like && target.vid.is_empty() {
        return Err(AppError::InvalidArgs("分集 ID 为空".into()));
    }
    let env = state.api_env();
    match action {
        Action::Like => interact::digg_video(&target.vid, &target.series_id, selected, &env).await,
        Action::Collect => interact::collect_series(&target.series_id, selected, &env).await,
        Action::Follow => {
            hongguo_service::follow_service::toggle(&state, &target.series_id, selected).await
        }
    }
}
