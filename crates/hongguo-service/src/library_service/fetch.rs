use super::{
    Kind,
    model::{Entry, Page, Query},
};
use crate::AppState;
use hongguo_api::{client::ApiEnv, interact, reservation};
use hongguo_common::{AppError, AppResult};

const MAX_MIXED_LIKES_PAGES: usize = 100;

pub async fn load(state: AppState, kind: Kind, query: Query) -> AppResult<Page> {
    let account = state
        .settings()
        .account
        .ok_or_else(|| AppError::Auth("登录后可查看个人片库".into()))?;
    let env = state.api_env();
    let mut page = match kind {
        Kind::Follows => follows(&state, &query).await?,
        Kind::Collections => match query.folder_id {
            Some(folder_id) => folder_items(folder_id, &query, &env).await?,
            None => Page {
                items: interact::fetch_bookshelf(&account.user_id, &env)
                    .await?
                    .into_iter()
                    .map(|item| Entry {
                        key: item.series_id.clone(),
                        series_id: item.series_id,
                        collection_book_id: Some(item.book_id),
                        collection_book_type: item.book_type,
                        content_type: item.content_type,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        },
        Kind::Liked => liked_videos(query.offset, &env).await?,
        Kind::Reservations => reservations(&query, &env).await?,
    };
    page.items = query.previous.merge(page.items);
    Ok(page)
}

pub async fn collection_folder_page(
    state: AppState,
    offset: i64,
) -> AppResult<interact::BookshelfFolderPage<interact::BookshelfFolder>> {
    let settings = state.settings();
    if settings.account.is_none() {
        return Err(AppError::Auth("登录后可查看收藏夹".into()));
    }
    interact::fetch_folders(offset, &state.api_env_from_settings(settings)).await
}

pub async fn create_collection_folder(
    state: AppState,
    name: String,
    publish: bool,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("登录后可新建收藏夹".into()));
    }
    interact::create_folder(&name, publish, &state.api_env()).await
}

pub async fn update_collection_folder(
    state: AppState,
    folder: i64,
    action: interact::FolderMembership,
    entries: Vec<Entry>,
) -> AppResult<()> {
    if state.settings().account.is_none() {
        return Err(AppError::Auth("登录后可编辑收藏夹".into()));
    }
    let mut seen = std::collections::HashSet::new();
    let items = entries
        .into_iter()
        .filter_map(|entry| {
            let id = match action {
                interact::FolderMembership::Add => entry.series_id,
                interact::FolderMembership::Remove => entry.collection_item_id?,
            };
            seen.insert(id.clone()).then_some((id, entry.content_type))
        })
        .collect::<Vec<_>>();
    interact::update_folder_items(folder, action, &items, &state.api_env()).await
}

async fn folder_items(id: i64, query: &Query, env: &ApiEnv) -> AppResult<Page> {
    let page = interact::fetch_folder_items(id, query.offset, env).await?;
    Ok(Page {
        folder: page.folder,
        items: page
            .items
            .into_iter()
            .map(|item| Entry {
                key: item.item_id.clone(),
                collection_item_id: Some(item.item_id),
                collection_folder_id: Some(id),
                collection_book_type: item.book_type,
                series_id: item.series_id,
                content_type: item.content_type,
                title: item.title,
                cover: item.cover,
                ..Default::default()
            })
            .collect(),
        offset: page.next_offset.unwrap_or(query.offset),
        has_more: page.next_offset.is_some(),
        ..Default::default()
    })
}

async fn follows(state: &AppState, query: &Query) -> AppResult<Page> {
    let page = crate::follow_service::list(state, &query.cursor).await?;
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

async fn liked_videos(mut offset: i64, env: &ApiEnv) -> AppResult<Page> {
    for _ in 0..MAX_MIXED_LIKES_PAGES {
        let page = interact::likes_verification::fetch_page(offset, env).await?;
        if page.items.is_empty()
            && let Some(next) = page.next_offset
        {
            offset = next;
            continue;
        }
        return Ok(Page {
            items: page
                .items
                .into_iter()
                .map(|item| Entry {
                    key: item.vid.clone(),
                    can_play: !item.series_id.is_empty(),
                    series_id: item.series_id,
                    title: item.series_title,
                    liked_vid: Some(item.vid),
                    digged_count: item.digged_count,
                    ..Default::default()
                })
                .collect(),
            offset: page.next_offset.unwrap_or(offset),
            has_more: page.next_offset.is_some(),
            ..Default::default()
        });
    }
    Err(AppError::Internal("liked videos pagination limit".into()))
}
