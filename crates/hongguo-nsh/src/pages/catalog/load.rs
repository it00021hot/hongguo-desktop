use super::model::{DEFAULT_CHANNEL, Query};
pub(super) use crate::player::resolve_start as target;
use hongguo_api::discover::{FeedItem, FeedPage};
use hongguo_common::AppResult;
use std::{collections::HashSet, sync::Arc};

pub(super) async fn feed(
    session: Arc<crate::bootstrap::Session>,
    query: Query,
) -> AppResult<FeedPage> {
    let state = &session.state;
    let mut page = hongguo_api::recommend::fetch_recommend_feed(
        &query.channel,
        &query.session,
        query.offset,
        &query.seen,
        &state.api_env(),
    )
    .await?;
    if query.channel == DEFAULT_CHANNEL && query.seen.is_empty() {
        let personal = session.personal.read(state).await?;
        let mut seeds = personal
            .history
            .iter()
            .map(|item| FeedItem {
                series_id: item.series_id.clone(),
                title: item.title.clone(),
                cover: item.cover.clone(),
                episode_cnt: u32::try_from(item.episode_cnt).unwrap_or(0),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        seeds.extend(personal.bookshelf.iter().map(|item| FeedItem {
            series_id: item.series_id.clone(),
            ..Default::default()
        }));
        let mut ids = HashSet::new();
        seeds.retain(|item| !item.series_id.is_empty() && ids.insert(item.series_id.clone()));
        seeds.extend(
            page.items
                .into_iter()
                .filter(|item| !item.series_id.is_empty() && ids.insert(item.series_id.clone())),
        );
        page.items = seeds;
    }
    Ok(page)
}
