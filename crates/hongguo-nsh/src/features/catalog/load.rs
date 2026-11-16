use super::model::{DEFAULT_CHANNEL, Query};
pub(super) use crate::features::player::resolve_start as target;
use hongguo_common::AppResult;
use hongguo_service::contracts::discover::{FeedItem, FeedPage};
use std::{collections::HashSet, sync::Arc};

pub(super) async fn feed(
    session: Arc<crate::bootstrap::Session>,
    query: Query,
) -> AppResult<FeedPage> {
    let state = &session.state;
    let mut page = hongguo_service::catalog_service::recommendations(
        &query.channel,
        &query.session,
        query.offset,
        &query.seen,
        state,
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
