use super::Kind;
use crate::{
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;

pub(super) fn frame(
    kind: Kind,
    toolbar: View,
    logged_in: Memo<bool>,
    nav: Navigation,
    body: impl Fn() -> View + 'static,
) -> View {
    view()
        .flex_1()
        .gap(move || {
            px(if matches!(kind, Kind::Collections | Kind::Liked) {
                12.0
            } else {
                16.0
            })
        })
        .p(move || px(if kind == Kind::Follows { 0.0 } else { 16.0 }))
        .child(toolbar)
        .child(when(
            move || kind == Kind::Reservations && !logged_in.get(),
            move || {
                crate::features::library::presentation::personal_login_required(
                    "reservation.loginRequired",
                    ui::Icon::Reservations,
                    move || nav.push(Route::Account),
                )
            },
        ))
        .child(when(logged_in, body))
}

pub(crate) mod personal_poster;
pub(crate) use personal_poster::{
    personal_poster, personal_poster_columns, personal_poster_pick, personal_poster_remove,
};

pub(crate) mod personal;
pub(crate) use personal::{
    personal_badge, personal_card, personal_cover, personal_date, personal_detail,
    personal_login_required, personal_pick, personal_publish_at, personal_score, personal_tab,
    personal_title,
};

pub(crate) mod follow_notices;
pub(crate) use follow_notices::{
    FollowNoticesState, NoticeAction, follow_login_required, follow_notice_feedback, follow_notices,
};

pub(crate) mod library;
pub(crate) use library::{
    TaskColumn, history_card, history_filter, history_open, history_progress, library_action,
    library_check, library_icon, library_search, task_badge, task_batch_bar, task_batch_close,
    task_cell, task_progress, task_table, task_table_row,
};
