use super::{
    Attachment, Editor, Scene, api, attachment,
    panel::{self, Intent, State},
    remote_model::{Feed, Tab},
};
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    pages::SessionHandle,
    ui,
};
use nsh_core::prelude::*;

pub(crate) fn picker(
    session: SessionHandle,
    nav: Navigation,
    editor: Editor,
    attachment: Signal<Option<Attachment>>,
    trigger: ElementRef,
) -> impl IntoElement {
    let open = signal(false);
    let feed = signal(Feed::default());
    let edits = session.emoji_collections();
    let notice = signal(SharedString::default());
    let mutation = signal(false);
    effect(move || {
        if !open.get() {
            feed.update(|feed| feed.select(Tab::System, String::new()));
        }
    });
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
    effect(move || {
        let account = account.get();
        if feed.with(|feed| feed.query.account.clone()) != account {
            feed.update(|feed| {
                feed.query.account = account;
                feed.reset();
            });
        }
    });
    effect(move || {
        let query = feed.with(|feed| feed.query.clone());
        if query.tab == Tab::Own
            && let Some(account) = query.account
        {
            let previous = feed.with_untracked(|feed| feed.items.clone());
            let next = edits.overlay(&account, previous.clone());
            if next != previous {
                feed.update(|feed| feed.items = next);
            }
        }
    });
    // Only the query changes trigger requests. Accepting a page never refetches it.
    let query = computed(move || {
        open.get()
            .then(|| feed.get())
            .filter(|feed| feed.accepts_load() && feed.query.account == account.get())
            .map(|feed| feed.query)
    });
    let loaded = resource(
        move || query.get(),
        move |query, _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                let page = api::load(state, query.clone()).await?;
                Ok((query, page))
            })
        },
    );
    effect(move || {
        if let Some(error) = loaded.error() {
            notice.set(error.text());
        }
        if !loaded.loading()
            && loaded.error().is_none()
            && let Some((query, mut page)) = loaded.get()
        {
            feed.update(|feed| {
                if query != feed.query {
                    return;
                }
                if query.tab == Tab::Own {
                    // Only a server page acknowledges an echo. The accumulated
                    // UI feed can itself contain that still-unindexed echo.
                    page.items =
                        edits.merge(query.account.as_deref().unwrap_or_default(), page.items);
                }
                feed.accept(&query, page);
            });
        }
    });
    let state = State {
        feed,
        notice,
        busy: computed(move || loaded.loading() || mutation.get()),
        failed: computed(move || loaded.error().is_some()),
    };
    let action = move |intent| match intent {
        Intent::Tab(tab) => {
            notice.set(SharedString::default());
            feed.update(|feed| {
                feed.select(tab, String::new());
                if tab == Tab::Own {
                    feed.items = edits.overlay(
                        feed.query.account.as_deref().unwrap_or_default(),
                        Vec::new(),
                    );
                }
            });
        }
        Intent::Search(keyword) => feed.update(|feed| feed.select(Tab::Search, keyword)),
        Intent::Insert(code) => {
            if let Err(error) = editor.insert(code) {
                notice.set(error);
            }
        }
        Intent::Pick(image, tab) => {
            attachment.set(Some(Attachment::Remote(
                image,
                if tab == Tab::Own {
                    Scene::Own
                } else {
                    Scene::ThirdParty
                },
            )));
            editor.reference.focus();
        }
        Intent::More => feed.update(Feed::more),
        Intent::Retry => loaded.refetch(),
        Intent::Login => {
            open.set(false);
            nav.push(Route::Account);
        }
        Intent::Collect(image, remove) => {
            if mutation.get_untracked() {
                return;
            }
            if account.get_untracked().is_none() {
                open.set(false);
                nav.push(Route::Account);
                return;
            }
            let identity = account.get_untracked();
            let image_key = super::remote_model::key(&image).to_owned();
            mutation.set(true);
            notice.set(SharedString::default());
            spawn(async move {
                if remove {
                    let answer = nsh_base::alert_dialog()
                        .title(tr("player.emoticon.removeConfirmTitle"))
                        .description(tr("player.emoticon.removeConfirmDesc"))
                        .cancel("cancel", tr("common.cancel"))
                        .destructive("remove", tr("common.confirm"))
                        .show()
                        .await;
                    match answer {
                        Ok(answer) if answer.action.as_deref() == Some("remove") => {}
                        Ok(_) => {
                            mutation.set(false);
                            return;
                        }
                        Err(error) => {
                            notice.set(error.to_string().into());
                            mutation.set(false);
                            return;
                        }
                    }
                }
                if identity != account.get_untracked() {
                    mutation.set(false);
                    return;
                }
                let state = session.with(|session| session.state.clone());
                let result = bridge::request(api::collect(state, image, remove)).await;
                mutation.set(false);
                if identity != account.get_untracked() {
                    return;
                }
                match result {
                    Ok(None) if !remove => notice.set(tr("player.emoticon.collectMissing")),
                    Ok(echo) => {
                        let account = identity.as_deref().unwrap_or_default();
                        if let Some(echo) = echo {
                            edits.remember(account, echo);
                        }
                        if remove {
                            edits.remove(account, image_key);
                        }
                        notice.set(tr(if remove {
                            "player.emoticon.removeDone"
                        } else {
                            "player.emoticon.addDone"
                        }));
                        feed.update(|feed| {
                            feed.reset();
                            if feed.query.tab == Tab::Own {
                                feed.items = edits.overlay(account, Vec::new());
                            }
                        });
                    }
                    Err(error) => notice.set(error.text()),
                }
            });
        }
        Intent::Upload => {
            if mutation.get_untracked() {
                return;
            }
            if account.get_untracked().is_none() {
                open.set(false);
                nav.push(Route::Account);
                return;
            }
            let identity = account.get_untracked();
            mutation.set(true);
            notice.set(SharedString::default());
            spawn(async move {
                let result = async {
                    let Some(image) = attachment::pick_local().await? else {
                        return Ok(None);
                    };
                    if identity != account.get_untracked() {
                        return Ok(None);
                    }
                    let state = session.with(|session| session.state.clone());
                    let echo = bridge::request(async move {
                        let image = image.prepare(state.clone()).await?.image;
                        api::collect(state, image, false).await
                    })
                    .await?;
                    Ok::<_, bridge::Failure>(Some(echo))
                }
                .await;
                mutation.set(false);
                if identity != account.get_untracked() {
                    return;
                }
                match result {
                    Ok(Some(Some(echo))) => {
                        let account = identity.as_deref().unwrap_or_default();
                        edits.remember(account, echo);
                        notice.set(tr("player.emoticon.uploadDone"));
                        feed.update(|feed| {
                            feed.reset();
                            if feed.query.tab == Tab::Own {
                                feed.items = edits.overlay(account, Vec::new());
                            }
                        });
                    }
                    Ok(Some(None)) => notice.set(tr("player.emoticon.collectMissing")),
                    Ok(None) => {}
                    Err(error) => notice.set(error.text()),
                }
            });
        }
    };
    ui::emoji_picker(
        open,
        ui::emoji_trigger(
            ui::Icon::Smile,
            move || tr("player.danmakuEmoji"),
            ui::EmojiTextStyle::Body,
        )
        .element_ref(&trigger)
        .selected(open),
        panel::panel(state, true, action).into_element(),
        move || editor.reference.focus(),
    )
}
