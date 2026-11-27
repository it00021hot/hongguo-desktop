mod load;
mod model;

use super::SessionHandle;
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_service::contracts::recommend::FeedTabMeta;
use model::{CHANNELS, Channel, DEFAULT_CHANNEL, FOLLOWING_CHANNEL, PREFETCH_REMAINING, Query};
use nsh_core::prelude::*;
use std::collections::HashMap;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    crate::shell::page(nav, move || tr("nav.home.title"))
        .header_visible(is_desktop())
        .child(content(session, nav))
}

fn content(session: SessionHandle, nav: Navigation) -> View {
    let channel = signal(DEFAULT_CHANNEL.to_owned());
    let query = signal(Some(Query {
        channel: DEFAULT_CHANNEL.into(),
        ..Default::default()
    }));
    let channels = signal(HashMap::<String, Channel>::new());
    let manual = signal(false);
    let identity = stored(session.identity());
    effect(move || {
        let next = session.identity();
        if identity.get() == next {
            return;
        }
        identity.replace(next);
        let signed_in = session.with(|session| session.state.settings().account.is_some());
        let next_channel = if !signed_in && channel.get_untracked() == FOLLOWING_CHANNEL {
            DEFAULT_CHANNEL.to_owned()
        } else {
            channel.get_untracked()
        };
        batch(|| {
            manual.set(false);
            channel.set(next_channel.clone());
            channels.set(HashMap::new());
            query.set(Some(Query {
                channel: next_channel,
                ..Default::default()
            }));
        });
    });
    let active = computed(move || {
        nav.routes()
            .with(|routes| routes.last() == Some(&Route::Catalog))
    });
    // Hydrate account-scoped resume history without adding it to the feed.
    resource(
        move || Some(session.identity()),
        move |_, _| {
            let session = session.with(|session| session.clone());
            bridge::request(async move { session.personal.read(&session.state).await.map(|_| ()) })
        },
    );
    let tabs = resource(
        move || Some(session.identity()),
        move |_, _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(
                async move { hongguo_service::catalog_service::feed_tabs(&state).await },
            )
        },
    );
    let available = computed(move || {
        session.identity();
        let signed_in = session.with(|session| session.state.settings().account.is_some());
        tabs.with(|tabs| {
            tabs.cloned()
                .filter(|tabs| !tabs.is_empty())
                .unwrap_or_else(|| {
                    CHANNELS
                        .into_iter()
                        .map(|(id, key)| FeedTabMeta {
                            tab_type: id.into(),
                            title: tr(key).to_string(),
                            bookstore_id: None,
                        })
                        .collect()
                })
                .into_iter()
                .filter(|tab| {
                    (tab.tab_type != FOLLOWING_CHANNEL || signed_in)
                        && CHANNELS.iter().any(|(id, _)| *id == tab.tab_type)
                })
                .collect::<Vec<_>>()
        })
    });
    let cached = move |id: &str| {
        channels.with_untracked(|channels| channels.get(id).is_some_and(Channel::has_items))
    };
    let select_channel = move |id: String| {
        batch(|| {
            if channel.get_untracked() != id || (!cached(&id) && query.get_untracked().is_none()) {
                let cached = cached(&id);
                manual.set(true);
                channel.set(id.clone());
                query.set((!cached).then_some(Query {
                    channel: id,
                    ..Default::default()
                }));
            }
            if !active.get_untracked() {
                nav.push(Route::Catalog);
            }
        });
    };
    let tab_pending = computed(move || manual.get() && result_pending(query.get(), &channel.get()));
    let toolbar_active = computed(move || {
        nav.routes()
            .with(|routes| matches!(routes.last(), Some(Route::Catalog | Route::Player(_, _))))
    });
    ui::toolbar_slot(toolbar_active, move || {
        channel_tabs(available, channel, tab_pending, select_channel)
    });
    // A tab switch may finish an in-flight batch behind the covering page.
    // It must not cancel or replay a finished batch when 首页 is shown again.
    let result = resource(
        move || query.get().map(|query| (query, session.identity())),
        move |(query, identity), _| {
            let state = session.with(|session| session.clone());
            let request = query.clone();
            bridge::request(async move {
                load::feed(state, query)
                    .await
                    .map(|page| (request, identity, page))
            })
        },
    );
    effect(move || {
        if result.state() != ResourceState::Ready {
            return;
        }
        let Some((request, identity, page)) = result.get() else {
            return;
        };
        if query.get_untracked().as_ref() != Some(&request) || session.identity() != identity {
            return;
        }
        log::info!(
            "[Catalog] page channel={} offset={} items={} hasMore={}",
            request.channel,
            request.offset,
            page.items.len(),
            page.has_more,
        );
        batch(|| {
            query.set(None);
            channels.update(|channels| channels.entry(request.channel).or_default().merge(page));
        });
    });
    effect(move || {
        if !active.get() || query.get_untracked().is_some() {
            return;
        }
        let id = channel.get_untracked();
        if !cached(&id) {
            // One new attempt when returning to a channel that had no content;
            // a successful empty response is not a warm cache hit.
            query.set(Some(Query {
                channel: id,
                ..Default::default()
            }));
        }
    });
    let selected = computed(move || {
        channels.with(|channels| {
            channels
                .get(&channel.get())
                .and_then(|page| page.items.get(page.index))
                .cloned()
        })
    });
    let ahead = computed(move || {
        if !active.get() {
            return Vec::new();
        }
        channels.with(|channels| {
            channels
                .get(&channel.get())
                .map(|page| {
                    page.items
                        .iter()
                        .skip(page.index + 1)
                        .take(hongguo_service::play_service::DEFAULT_PREFETCH_COUNT as usize)
                        .map(|item| item.series_id.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    resource(
        move || {
            let ids = ahead.get();
            (!ids.is_empty()).then(|| (ids, session.identity()))
        },
        move |(ids, _), _| {
            let session = session.with(|session| session.clone());
            bridge::request(async move {
                let personal = session.personal.recent(&session.state)?;
                hongguo_service::play_service::feed::prefetch(
                    &session.state,
                    ids,
                    personal.history.clone(),
                )
                .await
            })
        },
    );
    let explicit = signal(None::<(String, u32)>);
    effect(move || {
        if let Some((id, _)) = explicit.get()
            && selected.with(|item| item.as_ref().is_none_or(|item| item.series_id != id))
        {
            explicit.set(None);
        }
    });
    let target_key = computed(move || {
        selected.get().map(|item| {
            let episode = explicit
                .get()
                .filter(|(id, _)| *id == item.series_id)
                .map(|(_, episode)| episode);
            let vid = ((channel.get() == FOLLOWING_CHANNEL || item.highlight.is_some())
                && !item.vid.is_empty())
            .then_some(item.vid);
            (
                channel.get(),
                item.series_id,
                episode,
                vid,
                item.highlight,
                session.identity(),
            )
        })
    });
    let target = resource(
        move || target_key.get(),
        move |key, _| {
            let state = session.with(|session| session.clone());
            bridge::request(async move {
                let (_, id, episode, vid, highlight, _) = key.clone();
                if let Some(episode) = episode {
                    Ok((key, (id, episode)))
                } else {
                    load::target(state, id, vid, highlight)
                        .await
                        .map(|target| (key, target))
                }
            })
        },
    );
    let step = move |direction, is_manual| {
        if !active.get_untracked() {
            return false;
        }
        let id = channel.get_untracked();
        let mut changed = false;
        batch(|| {
            channels.update(|channels| {
                if let Some(page) = channels.get_mut(&id) {
                    changed = page.step(direction);
                }
            });
            if changed {
                manual.set(is_manual);
            }
        });
        changed
    };
    let feed_navigation = crate::features::player::navigation::FeedNavigation::new(step).on_goto(
        move |series, episode| {
            if !active.get_untracked() {
                return;
            }
            batch(|| {
                explicit.set(Some((series.series_id.clone(), episode)));
                let id = channel.get_untracked();
                channels.update(|channels| channels.entry(id).or_default().goto(series));
            });
        },
    );
    let context = stored(None::<(String, Option<String>)>);
    effect(move || {
        let next = (channel.get(), selected.get().map(|item| item.series_id));
        if context.replace(Some(next.clone())).as_ref() != Some(&next) {
            feed_navigation.invalidate();
        }
    });
    let feed_error = move || {
        if query.get().is_some() && result.state() == ResourceState::Errored {
            result.error()
        } else {
            None
        }
    };
    let target_error = move || {
        if selected.get().is_some() && target.state() == ResourceState::Errored {
            target.error()
        } else {
            None
        }
    };
    let cue = computed(move || {
        if !manual.get() || feed_error().is_some() || target_error().is_some() {
            return None;
        }
        let old = feed_navigation.playable().get();
        if tab_pending.get() {
            let current = channel.get();
            return available.with(|tabs| {
                tabs.iter()
                    .find(|tab| tab.tab_type == current)
                    .map(|tab| SharedString::from(tab.title.clone()))
            });
        }
        selected.with(|item| {
            let item = item.as_ref()?;
            (old.as_ref().is_none_or(|(id, _)| id != &item.series_id))
                .then(|| SharedString::from(item.title.clone()))
        })
    });
    let channel_empty = move || {
        query.get().is_none()
            && channels.with(|channels| {
                channels
                    .get(&channel.get())
                    .is_some_and(|page| !page.has_items())
            })
    };
    effect(move || {
        let ready = target.with(|value| {
            value
                .filter(|(key, _)| target_key.with(|current| current.as_ref() == Some(key)))
                .map(|(_, playable)| playable.clone())
        });
        let next = if channel_empty() {
            None
        } else {
            model::retain_playable(feed_navigation.playable().get_untracked(), ready)
        };
        if feed_navigation.playable().get_untracked() != next {
            let label = (manual.get_untracked() && next.is_some())
                .then(|| {
                    selected.with(|item| {
                        item.as_ref()
                            .map(|item| SharedString::from(item.title.clone()))
                    })
                })
                .flatten();
            feed_navigation.set_switch_label(label);
            feed_navigation.playable().set(next);
        }
    });
    let overlay = computed(move || {
        let id = feed_navigation
            .playable()
            .get()
            .map(|(id, _)| id)
            .unwrap_or_default();
        channels.with(|channels| {
            channels
                .get(&channel.get())
                .and_then(|page| {
                    page.items
                        .iter()
                        .find(|item| item.series_id == id)
                        .map(|item| crate::features::player::OverlayMeta {
                            series_id: item.series_id.clone(),
                            heat: item.heat_text.clone(),
                            season: item.season_tag.clone(),
                            badge: item.badge.clone(),
                        })
                })
                .unwrap_or_default()
        })
    });
    effect(move || {
        if !active.get() || query.get().is_some() {
            return;
        }
        let id = channel.get();
        let next = channels.with(|channels| {
            channels
                .get(&id)
                .filter(|page| {
                    page.has_more
                        && page.has_items()
                        && page.items.len().saturating_sub(page.index + 1) <= PREFETCH_REMAINING
                })
                .map(|page| Query {
                    channel: id,
                    offset: page.offset,
                    session: page.session.clone(),
                    seen: page
                        .items
                        .iter()
                        .map(|item| item.series_id.clone())
                        .collect(),
                })
        });
        if let Some(next) = next {
            query.set(Some(next));
        }
    });
    view()
        .flex_1()
        .min_h(px(0.0))
        .child(when(
            move || !is_desktop(),
            move || {
                crate::shell::mobile_catalog_header(
                    nav,
                    channel_tabs(available, channel, tab_pending, select_channel),
                )
            },
        ))
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .relative()
                .child(when(
                    move || feed_navigation.playable().get().is_some(),
                    move || {
                        let (id, episode) = feed_navigation
                            .playable()
                            .get_untracked()
                            .expect("playable player");
                        crate::features::player::embedded(
                            session,
                            nav,
                            id,
                            episode,
                            Route::Catalog,
                            overlay,
                            Some(feed_navigation),
                        )
                    },
                ))
                .child(when(
                    move || {
                        cue.get().is_none()
                            && model::cold_loading(
                                feed_navigation.playable().get().is_some(),
                                target.loading(),
                                result.loading(),
                                selected.get().is_some(),
                            )
                    },
                    move || {
                        view()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .pointer_events(PointerEvents::None)
                            .child(ui::loading::indicator("common.loading").size_6())
                    },
                ))
                .child(ui::loading::switch_cue(cue))
                .child(when(
                    move || feed_error().is_some() || target_error().is_some(),
                    move || {
                        view()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .child(super::error_state(
                                move || {
                                    target_error()
                                        .or_else(feed_error)
                                        .map_or_else(SharedString::default, |error| error.text())
                                },
                                move || {
                                    if target_error().is_some() {
                                        target.refetch();
                                    } else {
                                        result.refetch();
                                    }
                                },
                            ))
                    },
                ))
                .child(when(channel_empty, move || {
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(ui::status(move || {
                            tr(if channel.with(|id| id == FOLLOWING_CHANNEL) {
                                "feed.followEmpty"
                            } else {
                                "common.empty"
                            })
                        }))
                        .child(
                            ui::controls::button(
                                move || tr("common.retry"),
                                ui::controls::ButtonVariant::Secondary,
                            )
                            .on_press(move || select_channel(channel.get_untracked())),
                        )
                })),
        )
}

fn channel_tabs(
    available: Memo<Vec<FeedTabMeta>>,
    selected: Signal<String>,
    pending: Memo<bool>,
    on_select: impl Fn(String) + Copy + 'static,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .pointer_events(PointerEvents::Auto)
        .child(each(
            available,
            |tab: &FeedTabMeta| tab.tab_type.clone(),
            move |tab, _| {
                let label = move || {
                    let tab = tab.get();
                    if !tab.title.is_empty() {
                        return SharedString::from(tab.title);
                    }
                    CHANNELS
                        .iter()
                        .find(|(id, _)| *id == tab.tab_type)
                        .map_or_else(|| tab.tab_type.into(), |(_, key)| tr(key))
                };
                let is_selected = move || selected.get() == tab.with(|tab| tab.tab_type.clone());
                let button = crate::shell::mobile_channel_tab(label, is_selected, move || {
                    is_selected() && pending.get()
                });
                button.on_press(move || on_select(tab.get_untracked().tab_type))
            },
        ))
}

fn result_pending(query: Option<Query>, channel: &str) -> bool {
    query.is_some_and(|query| query.channel == channel && query.session.is_empty())
}
