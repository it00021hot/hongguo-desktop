use super::{
    ComposerContext,
    api::{self, Thread},
    model::Feed,
};
use crate::{
    bridge,
    i18n::tr,
    navigation::Route,
    ui::{self, ButtonVariant},
};
use nsh_core::prelude::*;

pub(super) fn composer(
    context: ComposerContext,
    kind: Stored<Thread>,
    feed: Signal<Feed>,
    parent_feed: Option<Signal<Feed>>,
    reply_to: Signal<Option<(String, String)>>,
    message: Signal<SharedString>,
) -> impl IntoElement {
    let ComposerContext {
        session,
        nav,
        target,
        logged_in: _,
        reviews: _,
        focus,
    } = context;
    let draft = signal(String::new());
    let review = matches!(kind.get(), Thread::Reviews);
    let stars = signal(0_u8);
    let editor = crate::emoji::Editor::new(draft, if review { 500 } else { 200 });
    let attachment = signal(None::<crate::emoji::Attachment>);
    let images = matches!(kind.get(), Thread::Comments);
    let pending = signal(false);
    let logged_in = computed(move || {
        session.identity();
        session.with(|session| session.state.settings().account.is_some())
    });
    let send = move || {
        if pending.get_untracked()
            || !logged_in.get_untracked()
            || review && super::review_model::review_score(stars.get_untracked()).is_none()
        {
            return;
        }
        let text = draft.get_untracked();
        let image = if images {
            attachment.get_untracked()
        } else {
            None
        };
        if text.trim().is_empty() && image.is_none() {
            return;
        }
        pending.set(true);
        message.set(SharedString::default());
        let state = session.with(|session| session.state.clone());
        let target = target.get();
        let thread = kind.get();
        let recipient = reply_to.get_untracked().map(|(id, _)| id);
        let rating = stars.get_untracked();
        spawn(async move {
            let result = bridge::request(api::send(
                state,
                target,
                thread.clone(),
                recipient,
                text,
                image,
                rating,
            ))
            .await;
            pending.set(false);
            match result {
                Ok(item) => {
                    feed.update(|feed| feed.accept_sent(item));
                    draft.set(String::new());
                    attachment.set(None);
                    reply_to.set(None);
                    if let (Some(parent), Some(parent_feed)) = (thread.parent(), parent_feed) {
                        parent_feed.update(|feed| feed.add_reply(parent));
                    }
                }
                Err(error) => message.set(error.text()),
            }
        });
    };
    if review {
        return super::review_form::form(
            editor,
            stars,
            pending,
            logged_in,
            super::review_form::Events {
                send: Box::new(send),
                login: Box::new(move || nav.push(Route::Account)),
            },
        );
    }
    let next = element_ref();
    let cancel = element_ref();
    let login = element_ref();
    view()
        .gap_2()
        .child(when(
            move || !logged_in.get(),
            move || {
                ui::button(move || tr("settings.loginAction"), ButtonVariant::Secondary)
                    .element_ref(&login)
                    .on_press(move || nav.push(Route::Account))
            },
        ))
        .child(when(
            move || reply_to.get().is_some(),
            move || {
                view()
                    .flex_row()
                    .gap_2()
                    .child(
                        text(move || {
                            tr("player.comments.replyPlaceholder").replace(
                                "{name}",
                                &reply_to.get().map_or_else(String::new, |(_, name)| name),
                            )
                        })
                        .flex_1(),
                    )
                    .child(
                        ui::button(move || tr("common.cancel"), ButtonVariant::Quiet)
                            .element_ref(&cancel)
                            .on_press(move || reply_to.set(None)),
                    )
            },
        ))
        .child(crate::emoji::sendbox(
            editor,
            pending,
            logged_in,
            images.then_some(attachment),
            if images {
                crate::emoji::picker(session, nav, editor, attachment, next).into_element()
            } else {
                crate::emoji::comment_picker(editor, next).into_element()
            },
            crate::emoji::SendEvents {
                send: Box::new(send),
                pick_image: Box::new(move || {
                    spawn(async move {
                        match crate::emoji::pick_local().await {
                            Ok(Some(image)) => attachment.set(Some(image)),
                            Ok(None) => {}
                            Err(error) => message.set(error.text()),
                        }
                    });
                }),
            },
            crate::emoji::SendOptions {
                allowed: computed(move || !review || stars.get() > 0),
                review,
                next,
                previous: Box::new(move || {
                    let stop = if reply_to.get_untracked().is_some() {
                        cancel
                    } else if !logged_in.get_untracked() {
                        login
                    } else {
                        focus.previous(feed)
                    };
                    stop.focus();
                }),
            },
        ))
}
