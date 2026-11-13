mod feed;
mod preferences;

pub(crate) use preferences::NoticePreferences;

use crate::{
    bridge,
    events::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_common::{AppError, AppResult, model::FollowNotice};
use hongguo_service::follow_service;
use nsh_core::prelude::*;

#[derive(Clone, PartialEq)]
enum Command {
    Check,
    Clear,
    Open(FollowNotice),
    Popup(bool),
}

#[derive(Clone)]
struct Outcome {
    message: Message,
    play: Option<(String, u32)>,
}

#[derive(Clone)]
enum Message {
    None,
    Checked(usize),
    Cleared,
}

impl Message {
    fn text(&self) -> SharedString {
        match self {
            Self::None => SharedString::default(),
            Self::Checked(0) => tr("follow.checkNone"),
            Self::Checked(count) => tr("follow.checkResult")
                .replace("{count}", &count.to_string())
                .into(),
            Self::Cleared => tr("follow.cleared"),
        }
    }
}

pub(super) fn build(session: SessionHandle, nav: Navigation, logged_in: Memo<bool>) -> View {
    let active = session.notice_preferences().active;
    let command = signal(None::<(u64, u64, Command)>);
    let sequence = stored(0_u64);
    let feedback = signal(SharedString::default());
    let feedback_owner = stored(session.identity());
    effect(move || {
        let identity = session.identity();
        let visible = nav
            .routes()
            .with(|routes| routes.last() == Some(&Route::Follows));
        if !logged_in.get()
            || !visible
            || command.with(|command| {
                command
                    .as_ref()
                    .is_some_and(|(_, owner, _)| *owner != identity)
            })
        {
            command.set(None);
            feedback.set(SharedString::default());
        }
        if feedback_owner.replace(identity) != identity {
            feedback.set(SharedString::default());
        }
    });
    let notifications = session.notifications();
    let preferences = session.notice_preferences();
    let application = session.with(|session| session.clone());
    let feed = crate::view_memory::cached("follow-notice-feed", move || {
        let visible = computed(move || {
            nav.routes()
                .with(|routes| routes.last() == Some(&Route::Follows))
        });
        feed::load(
            logged_in,
            visible,
            computed(move || session.identity()),
            computed(move || session.follow_notices()),
            move || {
                let state = session.with(|session| session.state.clone());
                bridge::request(async move {
                    hongguo_service::runtime::spawn_blocking(move || {
                        follow_service::notices(&state)
                    })
                    .await
                    .map_err(|error| AppError::Internal(error.to_string()))?
                })
            },
        )
    });
    let list = feed.result;
    let request = resource(
        move || {
            let identity = session.identity();
            if logged_in.get()
                && nav
                    .routes()
                    .with(|routes| routes.last() == Some(&Route::Follows))
            {
                command.get().filter(|(_, owner, _)| *owner == identity)
            } else {
                None
            }
        },
        move |(ticket, _, command), _| {
            let application = application.clone();
            async move {
                let result = match command {
                    Command::Popup(enabled) => popup(notifications, preferences, enabled).await,
                    command => bridge::request(execute(application, command))
                        .await
                        .map_err(|error| error.text()),
                };
                Ok::<_, std::convert::Infallible>((ticket, result))
            }
        },
    );
    effect(move || {
        if request.loading() {
            return;
        }
        let Some((received, outcome)) = request.get() else {
            return;
        };
        let Some((ticket, _, _)) = command.get_untracked() else {
            return;
        };
        if received != ticket {
            return;
        }
        command.set(None);
        session.notify_follow_notices();
        match outcome {
            Ok(outcome) => {
                feedback.set(outcome.message.text());
                if let Some((id, episode)) = outcome.play {
                    nav.push(Route::Player(id, episode));
                }
            }
            Err(error) => feedback.set(error),
        }
    });
    let items = feed.items;
    frame(
        active,
        computed(move || unread(&items.get())),
        logged_in,
        move || nav.push(Route::Account),
    )
    .child(ui::follow_notice_feedback(feedback))
    .child(when(
        move || logged_in.get() && !active.get(),
        move || super::content(session, nav, super::Kind::Follows),
    ))
    .child(when(
        move || logged_in.get() && active.get(),
        move || {
            view()
                .flex_1()
                .min_h(px(0.0))
                .child(when(
                    move || list.loading(),
                    move || ui::status(move || tr("common.loading")),
                ))
                .child(when(
                    move || !list.loading() && list.error().is_some(),
                    move || {
                        crate::pages::error_state(
                            move || {
                                list.error()
                                    .map_or_else(SharedString::default, |error| error.text())
                            },
                            move || list.refetch(),
                        )
                    },
                ))
                .child(ui::follow_notices(
                    ui::FollowNoticesState {
                        items,
                        busy: computed(move || request.loading()),
                        checking: computed(move || {
                            request.loading()
                                && command
                                    .with(|command| matches!(command, Some((_, _, Command::Check))))
                        }),
                        popup: session.notice_preferences().enabled,
                    },
                    move |action| {
                        if request.loading() {
                            return;
                        }
                        feedback.set(SharedString::default());
                        let next = sequence.get() + 1;
                        sequence.replace(next);
                        command.set(Some((
                            next,
                            session.identity(),
                            match action {
                                ui::NoticeAction::Check => Command::Check,
                                ui::NoticeAction::Clear => Command::Clear,
                                ui::NoticeAction::Open(notice) => Command::Open(notice),
                                ui::NoticeAction::Popup(enabled) => Command::Popup(enabled),
                            },
                        )));
                    },
                ))
        },
    ))
}

pub(super) fn tabs(active: Signal<bool>, unread: Memo<usize>) -> View {
    view()
        .flex_row()
        .gap_2()
        .child(
            ui::personal_tab(move || tr("follow.tabFollows"))
                .selected(move || !active.get())
                .on_press(move || active.set(false)),
        )
        .child(
            ui::personal_tab(move || tr("follow.tabNotices"))
                .flex_row()
                .items_center()
                .accessibility_label(move || tr("follow.tabNotices"))
                .selected(active)
                .on_press(move || active.set(true))
                .child(when(
                    move || unread.get() > 0,
                    move || {
                        view()
                            .test_id("follow-notices-unread")
                            .ml_1p5()
                            .px_1p5()
                            .py_0p5()
                            .rounded_full()
                            .bg(move || nsh_base::theme().destructive)
                            .child(
                                text(move || unread.get().to_string())
                                    .text_size(px(10.0))
                                    .text_color(move || {
                                        nsh_base::theme()
                                            .color("destructive-foreground")
                                            .expect("destructive foreground token")
                                    }),
                            )
                    },
                )),
        )
}

fn frame(
    active: Signal<bool>,
    unread: Memo<usize>,
    logged_in: Memo<bool>,
    on_login: impl Fn() + Copy + 'static,
) -> View {
    view()
        .relative()
        .flex_1()
        .p_4()
        .gap_4()
        .child(tabs(
            active,
            computed(move || if logged_in.get() { unread.get() } else { 0 }),
        ))
        .child(when(
            move || !logged_in.get(),
            move || ui::follow_login_required(on_login),
        ))
}

pub(super) fn unread(notices: &[FollowNotice]) -> usize {
    notices.iter().filter(|notice| !notice.read).count()
}

async fn execute(
    session: std::sync::Arc<crate::bootstrap::Session>,
    command: Command,
) -> AppResult<Outcome> {
    if session.state.settings().account.is_none() {
        return Err(AppError::Auth("登录后才能追更".into()));
    }
    let state = session.state.clone();
    let mut play = None;
    let message = match command {
        Command::Check => {
            let count = follow_service::check_updates(&state).await?;
            Message::Checked(count)
        }
        Command::Clear => {
            hongguo_service::runtime::spawn_blocking(move || follow_service::clear_notices(&state))
                .await
                .map_err(|error| AppError::Internal(error.to_string()))??;
            Message::Cleared
        }
        Command::Open(notice) => {
            if !notice.read {
                hongguo_service::runtime::spawn_blocking(move || {
                    follow_service::mark_notices_read(&state, &[notice.id])
                })
                .await
                .map_err(|error| AppError::Internal(error.to_string()))??;
            }
            play = Some(crate::player::resolve_start(session, notice.series_id).await?);
            Message::None
        }
        Command::Popup(_) => unreachable!("permission actions run on the UI owner"),
    };
    Ok(Outcome { message, play })
}

async fn popup(
    notifications: crate::notifications::Notifications,
    preferences: NoticePreferences,
    enabled: bool,
) -> Result<Outcome, SharedString> {
    if enabled {
        let allowed = notifications
            .request_permission()
            .await
            .map_err(|message| {
                SharedString::from(tr("follow.notificationFailed").replace("{message}", &message))
            })?;
        if !allowed {
            return Err(tr("follow.notificationDenied"));
        }
    }
    preferences.set(enabled).map_err(|message| {
        SharedString::from(tr("settings.saveFailed").replace("{message}", &message))
    })?;
    Ok(Outcome {
        message: Message::None,
        play: None,
    })
}
