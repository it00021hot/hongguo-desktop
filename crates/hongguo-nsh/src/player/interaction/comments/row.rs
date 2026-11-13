use super::{
    ComposerContext,
    api::{self, Thread},
    model::{Feed, Item},
    row_view, thread,
};
use crate::{bridge, i18n::tr};
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
        target,
        logged_in,
        reviews,
        focus,
    } = context;
    let pending = signal(false);
    let error = signal(SharedString::default());
    let replies = signal(false);
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
            logged_in,
            mine,
            reviews,
            reply_stop,
            delete_stop,
        },
        row_view::Events {
            like: Box::new(move || {
                if pending.get_untracked() || !logged_in.get_untracked() {
                    return;
                }
                let id = item.get_untracked().id().to_owned();
                let subject = api::Subject::for_item(reviews, &item.get_untracked());
                let mut updated = feed.get_untracked();
                let Some(before) = updated.toggle_like(&id) else {
                    return;
                };
                feed.set(updated);
                pending.set(true);
                error.set(SharedString::default());
                let state = session.with(|session| session.state.clone());
                spawn(async move {
                    let result =
                        bridge::request(api::like(state, id.clone(), !before.0, subject)).await;
                    pending.set(false);
                    if let Err(failure) = result {
                        feed.update(|feed| feed.restore_like(&id, before));
                        error.set(failure.text());
                    }
                });
            }),
            reply: Box::new(move || match item.get_untracked() {
                Item::Comment(_) => replies.update(|open| *open = !*open),
                Item::Reply(reply) => reply_to.set(Some((reply.reply_id, reply.user_name))),
            }),
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
                (if reviews { view() } else { view().h(px(360.0)) })
                    .pl_3()
                    .child(thread(
                        session,
                        nav,
                        target,
                        logged_in.get_untracked(),
                        if reviews {
                            Thread::ReviewReplies(parent)
                        } else {
                            Thread::Replies(parent)
                        },
                        Some(feed),
                    ))
                    .into_element()
            }),
        },
    )
}
