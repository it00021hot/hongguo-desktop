mod actions;
use hongguo_service::comments_service as api;
mod composer;
mod focus;
mod list_view;
use hongguo_service::comments_service::model;

mod review_form;
use hongguo_service::comments_service::review_model;
mod row;
mod row_view;
mod threads;

use super::Target;
use crate::{bridge, features::SessionHandle, i18n::tr, navigation::Navigation, ui};
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
    threads: threads::Threads,
    reply_state: Option<threads::ReplyThread>,
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
    thread_with_header(session, nav, target, Thread::Reviews, header, None)
}

pub(super) fn panel(
    session: SessionHandle,
    nav: Navigation,
    target: Stored<Target>,
    _logged_in: bool,
    open: Signal<bool>,
) -> impl IntoElement {
    let total = signal(0);
    let geometry = use_context::<crate::features::player::presentation::PlayerGeometry>()
        .expect("player geometry owns the comment panel");
    crate::features::player::presentation::comment_panel(open, total, geometry, move || {
        thread_with_header(session, nav, target, Thread::Comments, view(), Some(total))
    })
}

fn thread_with_header(
    session: SessionHandle,
    nav: Navigation,
    target: Stored<Target>,
    kind: Thread,
    header: View,
    total: Option<Signal<i64>>,
) -> View {
    let reviews = kind.is_review();
    let root_review = matches!(kind, Thread::Reviews);
    let kind = stored(kind);
    let feed = signal(Feed::default());
    if let Some(total) = total {
        effect(move || total.set(feed.with(|feed| feed.total)));
    }
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
    let threads = threads::Threads::new(feed, account, move |parent, cursor| {
        let state = session.with(|session| session.state.clone());
        let target = target.get();
        let kind = if reviews {
            Thread::ReviewReplies(parent)
        } else {
            Thread::Replies(parent)
        };
        bridge::request(api::load(state, target, kind, cursor))
    });
    let context = ComposerContext {
        session,
        nav,
        target,
        logged_in,
        reviews,
        focus: focus::Focus::new(),
        threads,
        reply_state: None,
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
    let form = move || composer(context, kind, feed, None, reply_to, message).into_element();
    let footer = move || {
        view()
            .gap_2()
            .child(when(
                move || loaded.loading() && feed.with(|feed| feed.items.is_empty()),
                move || comment_skeleton(3),
            ))
            .child(when(
                move || loaded.error().is_some(),
                move || {
                    crate::features::error_state(
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
                    let button = if root_review {
                        crate::features::player::presentation::comment_more(
                            move || tr("player.emoticon.loadMore"),
                            move || loaded.loading(),
                        )
                    } else {
                        more_button(feed, move || loaded.loading())
                    };
                    button
                        .element_ref(&context.focus.more)
                        .on_press(move || cursor.set(feed.get_untracked().cursor))
                },
            ))
            .child(ui::status(message))
    };
    let empty = move || {
        crate::features::player::presentation::comment_notice(
            move || {
                if loaded.loading() || loaded.error().is_some() {
                    SharedString::default()
                } else {
                    tr(if root_review {
                        "detail.commentsEmpty"
                    } else {
                        "player.comments.empty"
                    })
                }
            },
            reviews,
        )
    };
    let header = if root_review {
        view().child(header).child(
            crate::features::detail::presentation::detail_section()
                .gap_2()
                .child(form())
                .child(crate::features::player::presentation::review_summary(
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
        crate::features::detail::presentation::detail_section()
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
        .child(list_view::thread_list(
            feed,
            threads,
            header,
            list_footer,
            empty(),
            move |entry| {
                let content = match entry.get_untracked() {
                    threads::Entry::Root(_) => view().child(each(
                        move || match entry.get() {
                            threads::Entry::Root(item) => vec![item],
                            _ => Vec::new(),
                        },
                        |item: &Item| item.id().to_owned(),
                        move |item, _| row(context, item, feed, reply_to, stored(None)),
                    )),
                    threads::Entry::Reply { parent, .. } => {
                        let thread = threads
                            .get(&parent)
                            .expect("reply entry belongs to its thread");
                        let parent = stored(Some((feed, parent)));
                        crate::features::player::presentation::comment_thread_row(
                            each(
                                move || match entry.get() {
                                    threads::Entry::Reply { item, .. } => vec![item],
                                    _ => Vec::new(),
                                },
                                |item: &Item| item.id().to_owned(),
                                move |item, _| {
                                    row(
                                        ComposerContext {
                                            reply_state: Some(thread),
                                            ..context
                                        },
                                        item,
                                        thread.feed,
                                        thread.recipient,
                                        parent,
                                    )
                                },
                            ),
                            reviews,
                        )
                    }
                    threads::Entry::Footer(parent) => {
                        let footer = reply_footer(
                            threads.get(&parent).expect("footer belongs to its thread"),
                        );
                        if reviews {
                            footer
                                .pb_4()
                                .border_b_1()
                                .border_color(move || nsh_base::theme().border)
                        } else {
                            footer
                        }
                    }
                };
                if root_review {
                    crate::features::detail::presentation::detail_section()
                        .child(content)
                        .into_element()
                } else {
                    view().px_4().child(content).into_element()
                }
            },
        ))
        .child(when(
            move || !root_review,
            move || {
                view()
                    .p_3()
                    .border_t_1()
                    .border_color(move || {
                        nsh_base::theme()
                            .color("video-controls")
                            .expect("controls")
                            .opacity(0.1)
                    })
                    .child(form())
            },
        ))
}

fn more_button(feed: Signal<Feed>, pending: impl IntoReactive<bool>) -> nsh_base::ButtonElement {
    crate::features::player::presentation::comment_more(
        move || {
            let remaining = feed.with(|feed| (feed.total - feed.items.len() as i64).max(0));
            tr("player.comments.loadMore").replace("{n}", &remaining.to_string())
        },
        pending,
    )
}

fn reply_footer(thread: threads::ReplyThread) -> View {
    view()
        .pl_12()
        .gap_2()
        .child(when(
            move || thread.loaded.loading() && thread.feed.with(|feed| feed.items.is_empty()),
            move || comment_skeleton(1),
        ))
        .child(when(
            move || thread.loaded.error().is_some(),
            move || {
                crate::features::error_state(
                    move || {
                        thread
                            .loaded
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || thread.loaded.refetch(),
                )
            },
        ))
        .child(when(
            move || thread.feed.with(|feed| feed.has_more),
            move || {
                crate::features::player::presentation::review_link(
                    move || tr("player.emoticon.loadMore"),
                    move || thread.loaded.loading(),
                )
                .on_press(move || thread.cursor.set(thread.feed.get_untracked().cursor))
            },
        ))
        .child(ui::status(thread.message))
}

fn comment_skeleton(rows: usize) -> View {
    view()
        .role(AccessibilityRole::Group)
        .label(move || tr("common.loading"))
        .gap_4()
        .py_3()
        .children((0..rows).map(|_| {
            view()
                .flex_row()
                .gap_3()
                .items_start()
                .child(ui::loading::skeleton().size_8().rounded_full())
                .child(
                    view()
                        .gap_2()
                        .flex_1()
                        .child(ui::loading::skeleton().w(px(84.0)).h_3())
                        .child(ui::loading::skeleton().w(percentage(0.72)).h_3()),
                )
        }))
}
