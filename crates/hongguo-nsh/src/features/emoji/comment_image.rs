use super::api;
use crate::{
    bridge,
    features::SessionHandle,
    i18n::tr,
    navigation::{Navigation, Route},
};
use hongguo_service::contracts::emoticon::Emoticon;
use nsh_core::{element::ImageSrc, prelude::*};

pub(crate) fn comment_image(
    session: SessionHandle,
    nav: Navigation,
    image: ReadSignal<Emoticon>,
    notice: Signal<SharedString>,
) -> impl IntoElement {
    let pending = signal(false);
    let collections = session.emoji_collections();
    crate::features::emoji::presentation::emoji_context(
        crate::features::emoji::presentation::emoji_comment_image(move || {
            ImageSrc::from(hongguo_service::emoticon_service::attachment::preview(
                &image.get(),
            ))
        }),
        || false,
        move || {
            if pending.get_untracked() {
                return;
            }
            if session.with(|session| session.state.settings().account.is_none()) {
                nav.push(Route::Account);
                return;
            }
            pending.set(true);
            notice.set(SharedString::default());
            let state = session.with(|session| session.state.clone());
            let identity = session.identity();
            let account = state
                .settings()
                .account
                .map(|account| account.user_id)
                .unwrap_or_default();
            let image = image.get_untracked();
            spawn(async move {
                let result = bridge::request(api::collect(state, image, false)).await;
                pending.set(false);
                if identity != session.identity() {
                    return;
                }
                notice.set(match result {
                    Ok(Some(echo)) => {
                        collections.remember(&account, echo);
                        tr("player.emoticon.addDone")
                    }
                    Ok(None) => tr("player.emoticon.collectMissing"),
                    Err(error) => error.text(),
                });
            });
        },
    )
}
