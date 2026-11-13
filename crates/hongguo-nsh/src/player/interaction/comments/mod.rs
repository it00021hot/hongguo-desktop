mod api;
mod composer;
mod focus;
mod list_view;
mod model;
mod preview;
pub(crate) use preview::reviews_preview_with_header;
mod review_form;
mod review_model;
#[cfg(test)]
mod review_tests;
mod row;
mod row_view;
#[cfg(test)]
mod tests;

use super::Target;
use crate::{
    bridge,
    i18n::tr,
    navigation::Navigation,
    pages::SessionHandle,
    ui::{self, ButtonVariant},
};
use api::Thread;
use composer::composer;
use model::{Feed, Item};
use nsh_core::prelude::*;
use row::row;

#[derive(Clone, Copy)]
struct ComposerContext {
    session: SessionHandle,
    nav: Navigation,
    target: Stored<Target>,
    logged_in: Memo<bool>,
    reviews: bool,
    focus: focus::Focus,
}

pub(crate) fn reviews_with_header(
    session: SessionHandle,
    nav: Navigation,
    series_id: String,
    header: View,
) -> View {
    let target = stored(Target {
        series_id,
        vid: String::new(),
        title: String::new(),
        likes: 0,
        collections: 0,
        comments: 0,
    });
    thread_with_header(session, nav, target, Thread::Reviews, None, header)
}

pub(super) fn panel(
    session: SessionHandle,
    nav: Navigation,
    target: Stored<Target>,
    _logged_in: bool,
    open: Signal<bool>,
) -> impl IntoElement {
    ui::sheet(
        open,
        move || tr("player.comments.title"),
        move || thread(session, nav, target, false, Thread::Comments, None),
    )
}

fn thread(
    session: SessionHandle,
    nav: Navigation,
    target: Stored<Target>,
    _logged_in: bool,
    kind: Thread,
    parent_feed: Option<Signal<Feed>>,
) -> View {
    thread_with_header(session, nav, target, kind, parent_feed, view())
}

fn thread_with_header(
    session: SessionHandle,
    nav: Navigation,
    target: Stored<Target>,
    kind: Thread,
    parent_feed: Option<Signal<Feed>>,
    header: View,
) -> View {
    let reviews = kind.is_review();
    let root_review = matches!(kind, Thread::Reviews);
    let parent = stored(
        kind.parent()
            .and_then(|id| parent_feed.map(|feed| (feed, id.to_owned()))),
    );
    let kind = stored(kind);
    let feed = signal(Feed::default());
    let cursor = signal(String::new());
    let reply_to = signal(None::<(String, String)>);
    let message = signal(SharedString::default());
    let account = computed(move || {
        session.identity();
        session.with(|session| {
            session
                .state
                .settings()
                .account
                .map(|account| account.user_id)
        })
    });
    let previous = stored(account.get_untracked());
    effect(move || {
        let current = account.get();
        if previous.replace(current.clone()) != current {
            feed.set(Feed::default());
            cursor.set(String::new());
            reply_to.set(None);
            message.set(SharedString::default());
        }
    });
    let logged_in = computed(move || account.get().is_some());
    let context = ComposerContext {
        session,
        nav,
        target,
        logged_in,
        reviews,
        focus: focus::Focus::new(),
    };
    let loaded = resource(
        move || Some((cursor.get(), account.get())),
        move |(cursor, account), _| {
            let state = session.with(|session| session.state.clone());
            let target = target.get();
            let kind = kind.get();
            bridge::request(async move {
                let page = api::load(state, target, kind, cursor.clone()).await?;
                Ok((cursor, account, page))
            })
        },
    );
    effect(move || {
        if !loaded.loading()
            && loaded.error().is_none()
            && let Some((cursor, owner, page)) = loaded.get()
            && owner == account.get_untracked()
        {
            feed.update(|feed| feed.append(page, &cursor));
        }
    });
    let form = move || composer(context, kind, feed, parent_feed, reply_to, message).into_element();
    let footer = move || {
        view()
            .gap_2()
            .child(when(
                move || loaded.loading(),
                move || ui::status(move || tr("common.loading")),
            ))
            .child(when(
                move || loaded.error().is_some(),
                move || {
                    crate::pages::error_state(
                        move || {
                            loaded
                                .error()
                                .map_or_else(SharedString::default, |error| error.text())
                        },
                        move || loaded.refetch(),
                    )
                },
            ))
            .child(when(
                move || feed.with(|feed| feed.has_more),
                move || {
                    ui::button(
                        move || tr("player.emoticon.loadMore"),
                        ButtonVariant::Secondary,
                    )
                    .element_ref(&context.focus.more)
                    .disabled(move || loaded.loading())
                    .on_press(move || cursor.set(feed.get_untracked().cursor))
                },
            ))
            .child(ui::status(message))
    };
    let empty = move || {
        ui::status(move || {
            if loaded.loading() || loaded.error().is_some() {
                SharedString::default()
            } else {
                tr(if root_review {
                    "detail.commentsEmpty"
                } else {
                    "player.comments.empty"
                })
            }
        })
    };
    if reviews && !root_review {
        return view()
            .gap_2()
            .child(each(
                move || feed.get().items,
                |item: &Item| item.id().to_owned(),
                move |item, _| row(context, item, feed, reply_to, parent),
            ))
            .child(empty())
            .child(form())
            .child(footer());
    }
    let header = if root_review {
        view()
            .child(header)
            .child(
                ui::detail_section()
                    .gap_2()
                    .child(form())
                    .child(ui::review_summary(
                        move || {
                            feed.get()
                                .summary
                                .map_or_else(String::new, |summary| summary.score)
                        },
                        move || feed.get().summary.map_or(0, |summary| summary.count),
                        move || {
                            feed.get()
                                .summary
                                .map_or_else(Vec::new, |summary| summary.tags)
                        },
                    )),
            )
    } else {
        header
    };
    let list_footer = if root_review {
        ui::detail_section()
            .pb(move || {
                px(if window().width.0 < 768.0 {
                    144.0
                } else {
                    24.0
                })
            })
            .child(footer())
    } else {
        footer()
    };
    view()
        .element_ref(&context.focus.list)
        .role(nsh_core::AccessibilityRole::Group)
        .label(move || tr("player.comments.title"))
        .focusable(true)
        .flex_1()
        .min_h_0()
        .child(list_view::list(
            feed,
            header,
            list_footer,
            empty(),
            if root_review {
                "reviews-list"
            } else {
                "comments-list"
            },
            move |item| {
                let content = row(context, item, feed, reply_to, parent);
                if root_review {
                    ui::detail_section().child(content).into_element()
                } else {
                    content.into_element()
                }
            },
        ))
        .child(when(move || !root_review, form))
}
