use super::{
    Kind,
    model::{Entry, Query},
    rows::{self, Rows},
    toolbar::{self, Toolbar},
};
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::{
    calendar::CalendarItem,
    interact::{BookshelfEntry, FollowUpdateItem, InteractionItem},
};
use nsh_core::prelude::*;
#[cfg(test)]
mod program_tests;
mod reservations;

pub(super) fn page(nav: Navigation, kind: Kind) -> Screen {
    let screen = ui::page(nav, move || tr(kind.title()));
    let entries = match load(kind) {
        Ok(entries) => entries,
        Err(error) => return screen.child(ui::status(error)),
    };
    if kind == Kind::Reservations {
        return screen.child(reservations::build(nav, entries));
    }
    let entries = signal(entries);
    let query = signal(Query {
        online: true,
        ..Default::default()
    });
    let search = signal(String::new());
    let category = signal(0_i64);
    let busy = signal(false);
    let managing = signal(false);
    let selected = signal(Vec::<String>::new());
    let visible = computed(move || {
        let search = search.get().trim().to_lowercase();
        entries
            .get()
            .into_iter()
            .filter(|entry| {
                (search.is_empty()
                    || entry.title.to_lowercase().contains(&search)
                    || entry.series_id.to_lowercase().contains(&search))
                    && (category.get() == 0 || category.get() == entry.content_type)
                    && (kind != Kind::Reservations || entry.can_play == query.get().online)
            })
            .collect::<Vec<_>>()
    });
    let count = entries.with(Vec::len) as i64;
    let rows = Rows {
        kind,
        nav,
        items: visible,
        busy,
        managing,
        selected,
        has_more: computed(|| false),
        scroll_scope: computed(move || (0, query.get().online, search.get(), category.get())),
    };
    let remove = move |removed: Vec<Entry>| {
        entries.update(|entries| {
            entries.retain(|entry| !removed.iter().any(|removed| removed.key == entry.key))
        });
        selected
            .update(|keys| keys.retain(|key| !removed.iter().any(|removed| &removed.key == key)));
    };
    let feedback = signal(SharedString::default());
    let notices_open = signal(false);
    let notices = signal(Vec::<hongguo_common::model::FollowNotice>::new());
    screen.child(
        view()
            .relative()
            .flex_1()
            .p_4()
            .gap_4()
            .child(when(
                move || kind == Kind::Follows,
                move || {
                    super::notices::tabs(
                        notices_open,
                        computed(move || super::notices::unread(&notices.get())),
                    )
                },
            ))
            .child(
                view()
                    .flex_1()
                    .min_h(px(0.0))
                    .style_when(notices_open, |style| style.hidden())
                    .gap(move || {
                        px(if matches!(kind, Kind::Collections | Kind::Liked) {
                            12.0
                        } else {
                            16.0
                        })
                    })
                    .child(toolbar::build(Toolbar {
                        kind,
                        query,
                        search,
                        category,
                        online: signal(true),
                        busy,
                        managing,
                        selected,
                        counts: computed(move || Some((count, 0))),
                        logged_in: computed(|| true),
                    }))
                    .child(rows::build(
                        rows,
                        remove,
                        move |entry| nav.push(Route::Detail(entry.series_id)),
                        || {},
                    ))
                    .child(toolbar::batch(rows, remove, move |error| {
                        feedback.set(error)
                    }))
                    .child(ui::status(feedback)),
            )
            .child(ui::follow_notice_feedback(feedback))
            .child(when(notices_open, move || {
                view().flex_1().child(ui::follow_notices(
                    ui::FollowNoticesState {
                        items: computed(move || notices.get()),
                        busy: computed(move || busy.get()),
                        checking: computed(|| false),
                        popup: signal(false),
                    },
                    move |action| match action {
                        ui::NoticeAction::Check => {
                            notices.set(notice_samples());
                            feedback.set(tr("follow.checkResult").replace("{count}", "3").into());
                        }
                        ui::NoticeAction::Clear => {
                            notices.set(Vec::new());
                            feedback.set(tr("follow.cleared"));
                        }
                        ui::NoticeAction::Open(notice) => {
                            notices.update(|items| {
                                for item in items.iter_mut().filter(|item| item.id == notice.id) {
                                    item.read = true;
                                }
                            });
                            nav.push(Route::Detail(notice.series_id));
                        }
                        ui::NoticeAction::Popup(_) => feedback.set(tr("follow.notificationDenied")),
                    },
                ))
            })),
    )
}

fn load(kind: Kind) -> Result<Vec<Entry>, String> {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/nsh-layout-fixtures/"
    );
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../preview/fixtures/personal.json"))
            .map_err(|error| format!("personal preview JSON: {error}"))?;
    let decode_error = |error| format!("personal preview fields: {error}");
    let mut entries: Vec<Entry> = match kind {
        Kind::Follows => {
            serde_json::from_value::<Vec<FollowUpdateItem>>(data["followRows"].clone())
                .map_err(decode_error)?
                .into_iter()
                .map(|item| Entry {
                    key: item.series_id.clone(),
                    series_id: item.series_id,
                    title: item.title,
                    episode_count: item.episode_cnt,
                    watched_episode: item.latest_watched_vid_index,
                    finished: Some(item.finished),
                    can_play: true,
                    ..Default::default()
                })
                .collect()
        }
        Kind::Reservations => {
            serde_json::from_value::<Vec<CalendarItem>>(data["reservationRows"].clone())
                .map_err(decode_error)?
                .into_iter()
                .map(|item| Entry {
                    key: item.series_id.clone(),
                    series_id: item.series_id.clone(),
                    title: item.title.clone(),
                    summary: item.description.clone(),
                    episode_count: item.episode_cnt.into(),
                    can_play: item.is_online,
                    reservation: Some(item),
                    ..Default::default()
                })
                .collect()
        }
        Kind::Collections => {
            serde_json::from_value::<Vec<BookshelfEntry>>(data["collectionRows"].clone())
                .map_err(decode_error)?
                .into_iter()
                .map(|item| Entry {
                    key: item.series_id.clone(),
                    series_id: item.series_id,
                    content_type: item.content_type,
                    can_play: true,
                    ..Default::default()
                })
                .collect()
        }
        Kind::Liked => serde_json::from_value::<Vec<InteractionItem>>(data["likedRows"].clone())
            .map_err(decode_error)?
            .into_iter()
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
    };
    let metadata = serde_json::from_value::<Vec<hongguo_common::model::Series>>(
        data["personalSeries"].clone(),
    )
    .map_err(decode_error)?;
    for entry in &mut entries {
        entry.cover = format!("{root}player-frame.png");
        if let Some(series) = metadata
            .iter()
            .find(|series| series.series_id == entry.series_id)
        {
            entry.title.clone_from(&series.title);
            entry.episode_count = series.episode_count.into();
        }
    }
    Ok(entries)
}

pub(super) fn notice_samples() -> Vec<hongguo_common::model::FollowNotice> {
    use hongguo_common::model::{FollowNotice, NOTICE_KIND_FOLLOW, NOTICE_KIND_ONLINE};
    [
        (
            1,
            "notice-first",
            "归途有风",
            "追的剧更新了，新增 2 集（共 24 集）",
            NOTICE_KIND_FOLLOW,
            false,
        ),
        (
            2,
            "notice-second",
            "日光之下",
            "追的剧更新了，新增 1 集（共 40 集）",
            NOTICE_KIND_FOLLOW,
            true,
        ),
        (
            3,
            "notice-third",
            "城市来信",
            "预约的剧已上线，共 60 集",
            NOTICE_KIND_ONLINE,
            false,
        ),
    ]
    .into_iter()
    .map(|(id, series, title, detail, kind, read)| FollowNotice {
        id,
        series_id: series.into(),
        title: title.into(),
        detail: detail.into(),
        kind: kind.into(),
        at: 1_794_384_000 + id,
        read,
    })
    .collect()
}
