use super::{CalendarCursor, CalendarFilteredPage, CalendarPanel, FilteredPage, Query};
use hongguo_common::{AppError, AppResult};
use hongguo_service::{AppState, catalog_service};
use std::collections::HashSet;

pub(super) async fn load(
    query: Query,
    panel: Option<&CalendarPanel>,
    state: &AppState,
) -> AppResult<FilteredPage> {
    let date = if query.date.is_empty() {
        panel.map_or("", |panel| panel.default_date.as_str())
    } else {
        query.date.as_str()
    };
    let selection = panel
        .and_then(|panel| panel.row.as_ref())
        .and_then(|row| {
            query
                .filter
                .as_ref()
                .filter(|id| row.options.iter().any(|option| &option.id == *id))
                .map(|id| (row.key.clone(), id.clone()))
        })
        .into_iter()
        .collect();
    let mut next = query.previous.as_ref().map(|page| page.cursor.clone());
    let mut dates = panel
        .map(|panel| (panel.dates.clone(), panel.default_date.clone()))
        .or_else(|| {
            query
                .previous
                .as_ref()
                .map(|result| (result.page.dates.clone(), result.page.default_date.clone()))
        });
    let mut items = query
        .previous
        .map_or_else(Vec::new, |result| result.page.items);
    let mut seen: HashSet<_> = items.iter().map(|item| item.series_id.clone()).collect();

    loop {
        let previous_offset = next.as_ref().map_or(0, |cursor| cursor.offset);
        let CalendarFilteredPage { mut page, cursor } = if panel.is_some() {
            catalog_service::filtered_calendar(date, &selection, next.as_ref(), state).await?
        } else {
            let page = catalog_service::calendar(
                (!date.is_empty()).then_some(date),
                next.as_ref()
                    .map(|cursor| (cursor.offset, cursor.session.as_str())),
                state,
            )
            .await?;
            let cursor = CalendarCursor {
                offset: page.next_offset,
                subscribe_offset: None,
                session: page.session_id.clone(),
            };
            CalendarFilteredPage { page, cursor }
        };
        let dates = dates.get_or_insert_with(|| (page.dates.clone(), page.default_date.clone()));
        page.items
            .retain(|item| seen.insert(item.series_id.clone()));
        if page.has_more && cursor.offset <= previous_offset {
            return Err(AppError::Media("上新日历分页位置未前进".into()));
        }
        if !page.items.is_empty() || !page.has_more {
            (page.dates, page.default_date) = dates.clone();
            items.append(&mut page.items);
            page.items = items;
            return Ok(FilteredPage {
                page,
                cursor,
                filter: query.filter,
                date: query.date,
                identity: query.identity,
            });
        }
        next = Some(cursor);
    }
}
