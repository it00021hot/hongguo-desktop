mod load;
mod model;

use super::SessionHandle;
use crate::{
    bridge,
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use hongguo_api::recommend::FeedTabMeta;
use model::{CHANNELS, Channel, DEFAULT_CHANNEL, FOLLOW_CHANNEL, PREFETCH_REMAINING, Query};
use nsh_core::prelude::*;
use std::collections::HashMap;

pub(super) fn page(session: SessionHandle, nav: Navigation) -> Screen {
    ui::page(nav, move || tr("nav.home.title"))
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
    let identity = stored(session.identity());
    effect(move || {
        let next = session.identity();
        if identity.get() == next {
            return;
        }
        identity.replace(next);
        batch(|| {
            channels.set(HashMap::new());
            query.set(Some(Query {
                channel: channel.get_untracked(),
                ..Default::default()
            }));
        });
    });
    let active = computed(move || {
        nav.routes()
            .with(|routes| routes.last() == Some(&Route::Catalog))
    });
    let tabs = resource(
        move || Some(session.identity()),
        move |_, _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                hongguo_api::recommend::fetch_feed_tabs(&state.api_env()).await
            })
        },
    );
    let available = computed(move || {
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
                .filter(|tab| tab.tab_type != FOLLOW_CHANNEL)
                .collect::<Vec<_>>()
        })
    });
    let select_channel = move |id: String| {
        if channel.get_untracked() == id {
            return;
        }
        if !active.get_untracked() {
            nav.push(Route::Catalog);
        }
        let cached = channels.with(|channels| channels.contains_key(&id));
        batch(|| {
            channel.set(id.clone());
            query.set((!cached).then_some(Query {
                channel: id,
                ..Default::default()
            }));
        });
    };
    let toolbar_active = computed(move || {
        nav.routes()
            .with(|routes| matches!(routes.last(), Some(Route::Catalog | Route::Player(_, _))))
    });
    ui::toolbar_slot(toolbar_active, move || {
        channel_tabs(available, channel, select_channel)
    });
    let result = resource(
        move || {
            active
                .get()
                .then(|| query.get())
                .flatten()
                .map(|query| (query, session.identity()))
        },
        move |(query, _), _| {
            let state = session.with(|session| session.clone());
            let id = query.channel.clone();
            bridge::request(async move { load::feed(state, query).await.map(|page| (id, page)) })
        },
    );
    effect(move || {
        if result.loading() {
            return;
        }
        if let Some((id, page)) = result.get() {
            channels.update(|channels| channels.entry(id).or_default().merge(page));
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
    let explicit = signal(None::<(String, u32)>);
    effect(move || {
        if let Some((id, _)) = explicit.get()
            && selected.with(|item| item.as_ref().is_none_or(|item| item.series_id != id))
        {
            explicit.set(None);
        }
    });
    let target = resource(
        move || {
            selected.get().map(|item| {
                let episode = explicit
                    .get()
                    .filter(|(id, _)| *id == item.series_id)
                    .map(|(_, episode)| episode);
                (item.series_id, episode)
            })
        },
        move |(id, episode), _| {
            let state = session.with(|session| session.clone());
            bridge::request(async move {
                if let Some(episode) = episode {
                    Ok((id, episode))
                } else {
                    load::target(state, id).await
                }
            })
        },
    );
    let step = move |direction| {
        if !active.get_untracked() {
            return false;
        }
        let id = channel.get_untracked();
        let mut changed = false;
        channels.update(|channels| {
            if let Some(page) = channels.get_mut(&id) {
                changed = page.step(direction);
            }
        });
        changed
    };
    let feed_navigation =
        crate::player::navigation::FeedNavigation::new(step).on_goto(move |series, episode| {
            if !active.get_untracked() {
                return;
            }
            batch(|| {
                explicit.set(Some((series.series_id.clone(), episode)));
                let id = channel.get_untracked();
                channels.update(|channels| channels.entry(id).or_default().goto(series));
            });
        });
    let context = stored(None::<(String, Option<String>)>);
    effect(move || {
        let next = (channel.get(), selected.get().map(|item| item.series_id));
        if context.replace(Some(next.clone())).as_ref() != Some(&next) {
            feed_navigation.invalidate();
        }
    });
    effect(move || {
        if !active.get() || result.loading() || result.error().is_some() {
            return;
        }
        let id = channel.get();
        let next = channels.with(|channels| {
            channels
                .get(&id)
                .filter(|page| {
                    page.has_more
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
            move || ui::mobile_catalog_header(channel_tabs(available, channel, select_channel)),
        ))
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .relative()
                .child(each(
                    move || target.with(|target| target.cloned().into_iter().collect::<Vec<_>>()),
                    |target: &(String, u32)| target.0.clone(),
                    move |target, _| {
                        let (id, episode) = target.get_untracked();
                        crate::player::embedded(
                            session,
                            nav,
                            id,
                            episode,
                            Route::Catalog,
                            computed(move || {
                                selected.with(|item| {
                                    item.as_ref()
                                        .map(|item| crate::player::OverlayMeta {
                                            series_id: item.series_id.clone(),
                                            heat: item.heat_text.clone(),
                                            season: item.season_tag.clone(),
                                            badge: item.badge.clone(),
                                        })
                                        .unwrap_or_default()
                                })
                            }),
                            Some(feed_navigation),
                        )
                    },
                ))
                .child(when(
                    move || target.loading() || (result.loading() && selected.get().is_none()),
                    move || {
                        view()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .pointer_events(PointerEvents::None)
                            .child(ui::status(move || tr("common.loading")))
                    },
                ))
                .child(when(
                    move || result.error().is_some() || target.error().is_some(),
                    move || {
                        view()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .child(super::error_state(
                                move || {
                                    target
                                        .error()
                                        .or_else(|| result.error())
                                        .map_or_else(SharedString::default, |error| error.text())
                                },
                                move || {
                                    if target.error().is_some() {
                                        target.refetch();
                                    } else {
                                        result.refetch();
                                    }
                                },
                            ))
                    },
                ))
                .child(when(
                    move || {
                        !result.loading() && result.error().is_none() && selected.get().is_none()
                    },
                    move || ui::status(move || tr("common.empty")),
                )),
        )
}

fn channel_tabs(
    available: Memo<Vec<FeedTabMeta>>,
    selected: Signal<String>,
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
                let button = if is_desktop() {
                    ui::header_tab(label).selected(is_selected)
                } else {
                    ui::mobile_channel_tab(label, is_selected)
                };
                button.on_press(move || on_select(tab.get_untracked().tab_type))
            },
        ))
}
