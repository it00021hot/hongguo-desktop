use super::{
    ComposerContext, actions,
    api::{self, Thread},
    composer::composer,
    model::{Feed, Item},
    row_view,
};
use crate::{bridge, i18n::tr, ui};
use nsh_core::prelude::*;

pub(super) fn row(
    context: ComposerContext,
    item: ReadSignal<Item>,
    feed: Signal<Feed>,
    reply_to: Signal<Option<(String, String)>>,
    parent: Stored<Option<(Signal<Feed>, String)>>,
) -> impl IntoElement {
    let ComposerContext {
        session,
        nav,
        target: _,
        logged_in,
        reviews,
        focus,
        reply_state,
        threads,
    } = context;
    let action = threads.action(item.get_untracked().id());
    let pending = action.pending;
    let error = action.error;
    let own_thread = threads.get(item.get_untracked().id());
    let replies = own_thread.map_or_else(|| signal(false), |thread| thread.expanded);
    let editing = own_thread.map_or_else(|| signal(false), |thread| thread.editing);
    let mine = computed(move || {
        session.identity();
        session.with(|session| {
            session
                .state
                .settings()
                .account
                .is_some_and(|account| account.user_id == item.get().user_id())
        })
    });
    let (reply_stop, delete_stop) =
        focus.register(item.get_untracked().id().to_owned(), mine, pending);
    row_view::row(
        row_view::State {
            item,
            pending,
            error,
            replies,
            editing,
            mine,
            reviews,
            reply_stop,
            delete_stop,
        },
        row_view::Events {
            like: Box::new(move || {
                let id = item.get_untracked().id().to_owned();
                let subject = api::Subject::for_item(reviews, &item.get_untracked());
                let state = session.with(|session| session.state.clone());
                threads.run(move || {
                    actions::like(
                        feed,
                        id,
                        logged_in.get_untracked(),
                        threads.account,
                        pending,
                        error,
                        move |id, selected| async move {
                            bridge::request(api::like(state, id, selected, subject))
                                .await
                                .map_err(|failure| failure.text())
                        },
                    )
                });
            }),
            reply: Box::new(move || match item.get_untracked() {
                Item::Comment(_) => {
                    if let Some(thread) = own_thread {
                        thread.recipient.set(None);
                    }
                    editing.update(|open| *open = !*open);
                }
                Item::Reply(reply) => {
                    reply_to.set(Some((reply.reply_id, reply.user_name)));
                    if let Some(state) = reply_state {
                        state.editing.set(true);
                    }
                }
            }),
            toggle_replies: Box::new(move || replies.update(|open| *open = !*open)),
            delete: Box::new(move || {
                if pending.get_untracked() {
                    return;
                }
                pending.set(true);
                let value = item.get_untracked();
                let id = value.id().to_owned();
                let owner = value.user_id().to_owned();
                let subject = api::Subject::for_item(reviews, &value);
                let title = tr(if matches!(value, Item::Reply(_)) {
                    "player.comments.deleteReplyTitle"
                } else {
                    "player.comments.deleteCommentTitle"
                });
                let confirmation = nsh_base::alert_dialog()
                    .title(title)
                    .description(tr("player.comments.deleteDesc"))
                    .cancel("cancel", tr("common.cancel"))
                    .destructive("delete", tr("common.delete"))
                    .show();
                spawn(async move {
                    match confirmation.await {
                        Ok(answer) if answer.action.as_deref() == Some("delete") => {
                            let state = session.with(|session| session.state.clone());
                            match bridge::request(api::delete(state, id.clone(), owner, subject))
                                .await
                            {
                                Ok(()) => {
                                    feed.update(|feed| feed.remove(&id));
                                    parent.with(|parent| {
                                        if let Some((feed, id)) = parent {
                                            feed.update(|feed| feed.remove_reply(id));
                                        }
                                    });
                                }
                                Err(failure) => error.set(failure.text()),
                            }
                        }
                        Ok(_) => {}
                        Err(failure) => error.set(failure.to_string().into()),
                    }
                    pending.set(false);
                });
            }),
            image: Box::new(move |image| {
                crate::emoji::comment_image(session, nav, image, error).into_element()
            }),
            replies: Box::new(move || {
                let parent = item.get_untracked().id().to_owned();
                let Some(thread) = own_thread else {
                    return view().into_element();
                };
                let kind = stored(if reviews {
                    Thread::ReviewReplies(parent)
                } else {
                    Thread::Replies(parent)
                });
                view()
                    .child(when(thread.editing, move || {
                        view()
                            .child(composer(
                                ComposerContext {
                                    reply_state: Some(thread),
                                    ..context
                                },
                                kind,
                                thread.feed,
                                Some(feed),
                                thread.recipient,
                                thread.message,
                            ))
                            .child(ui::status(thread.message))
                    }))
                    .into_element()
            }),
        },
    )
}
