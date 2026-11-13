use super::{
    Kind,
    model::{Entry, Page, Query, merge_page},
};
use crate::bridge::Failure;
use hongguo_api::{client::ApiEnv, detail, interact, reservation};
use hongguo_common::{AppError, AppResult};
use hongguo_service::AppState;

const METADATA_BATCH_SIZE: usize = 6;

pub(super) async fn load(state: AppState, kind: Kind, query: Query) -> AppResult<Page> {
    let account = state
        .settings()
        .account
        .ok_or_else(|| AppError::Auth("登录后可查看个人片库".into()))?;
    let env = state.api_env();
    let mut page = match kind {
        Kind::Follows => follows(&state, &query).await?,
        Kind::Collections => Page {
            items: interact::fetch_bookshelf(&account.user_id, &env)
                .await?
                .into_iter()
                .map(|item| Entry {
                    key: item.series_id.clone(),
                    series_id: item.series_id,
                    content_type: item.content_type,
                    can_play: true,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
        Kind::Liked => Page {
            items: interact::fetch_interaction_state(&env)
                .await?
                .items
                .into_iter()
                .filter(|item| item.user_digg)
                .map(|item| Entry {
                    key: item.vid.clone(),
                    series_id: item.series_id,
                    title: item.series_title,
                    liked_vid: Some(item.vid),
                    digged_count: item.digged_count,
                    can_play: true,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
        Kind::Reservations => reservations(&query, &env).await?,
    };
    if matches!(kind, Kind::Collections | Kind::Liked) {
        page.warning = enrich(&state, &env, &mut page.items).await?;
    }
    page.items = merge_page(query.previous, page.items);
    Ok(page)
}

async fn follows(state: &AppState, query: &Query) -> AppResult<Page> {
    let page = hongguo_service::follow_service::list(state, &query.cursor).await?;
    if page.has_more && (page.cursor.is_empty() || page.cursor == query.cursor) {
        return Err(AppError::Media("追更列表分页游标未前进".into()));
    }
    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(|item| Entry {
                key: item.series_id.clone(),
                series_id: item.series_id,
                title: item.title,
                cover: item.cover,
                episode_count: item.episode_cnt,
                watched_episode: item.latest_watched_vid_index,
                finished: Some(item.finished),
                can_play: true,
                ..Default::default()
            })
            .collect(),
        cursor: page.cursor,
        has_more: page.has_more,
        ..Default::default()
    })
}

async fn reservations(query: &Query, env: &ApiEnv) -> AppResult<Page> {
    let session = (!query.session.is_empty()).then_some(query.session.as_str());
    let page =
        reservation::fetch_reservations_page(query.online, query.offset, session, env).await?;
    if page.has_more && page.next_offset <= query.offset {
        return Err(AppError::Media("预约列表分页位置未前进".into()));
    }
    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(|item| Entry {
                key: item.series_id.clone(),
                series_id: item.series_id.clone(),
                title: item.title.clone(),
                cover: item.cover.clone(),
                summary: item.description.clone(),
                episode_count: item.episode_cnt.into(),
                can_play: item.is_online,
                reservation: Some(item),
                ..Default::default()
            })
            .collect(),
        offset: page.next_offset,
        session: page.session_id,
        has_more: page.has_more,
        reservation_counts: Some((page.online_total, page.offline_total)),
        ..Default::default()
    })
}

async fn enrich(
    state: &AppState,
    env: &ApiEnv,
    entries: &mut [Entry],
) -> AppResult<Option<Failure>> {
    let ids: Vec<_> = entries
        .iter()
        .map(|entry| entry.series_id.clone())
        .collect();
    let store = state.store.clone();
    let cached = hongguo_service::runtime::spawn_blocking(move || {
        ids.into_iter()
            .map(|id| store.series_by_id(&id))
            .collect::<AppResult<Vec<_>>>()
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    for series in cached.into_iter().flatten() {
        for entry in entries
            .iter_mut()
            .filter(|entry| entry.series_id == series.series_id)
        {
            entry.title.clone_from(&series.title);
            entry.cover.clone_from(&series.cover);
            entry.episode_count = series.episode_count.into();
        }
    }
    let mut missing: Vec<_> = entries
        .iter()
        .filter(|entry| entry.title.is_empty() || entry.cover.is_empty())
        .map(|entry| entry.series_id.clone())
        .collect();
    missing.sort();
    missing.dedup();
    for batch in missing.chunks(METADATA_BATCH_SIZE) {
        let metadata = match detail::fetch_episode_lists_batch(batch, env).await {
            Ok(metadata) => metadata,
            Err(error) => return Ok(Some(Failure::from(error))),
        };
        for series in metadata {
            for entry in entries
                .iter_mut()
                .filter(|entry| entry.series_id == series.series_id)
            {
                entry.title.clone_from(&series.title);
                entry.cover.clone_from(&series.cover);
                entry.episode_count = series.episodes.len() as i64;
            }
        }
    }
    Ok(None)
}
