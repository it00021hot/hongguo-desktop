use crate::{
    bridge,
    i18n::tr,
    navigation::Navigation,
    pages::{SessionHandle, series_feed},
    ui,
};
use hongguo_api::{
    detail::{RelatedItem, RelatedSeries},
    rank::RankItem,
};
use nsh_core::prelude::*;
use std::collections::{HashMap, HashSet};

type Source = Resource<RelatedSeries, bridge::Failure>;

pub(super) fn preview() -> Source {
    let items = crate::pages::fixtures::catalog()
        .rank
        .items
        .into_iter()
        .take(6)
        .map(|item| RelatedItem {
            series_id: item.series_id,
            title: item.title,
            cover: crate::pages::fixtures::FRAME_PATH.into(),
            tag: item.season,
            score: item.score,
            play_cnt: item.play_cnt,
            episode_cnt: item.episode_cnt,
            video_desc: item.description,
        })
        .collect::<Vec<_>>();
    let seed = RelatedSeries {
        works: items[..2].to_vec(),
        guess: items[2..].to_vec(),
    };
    resource(
        || Some(()),
        move |_, _| std::future::ready(Ok(seed.clone())),
    )
}

pub(super) fn load(session: SessionHandle, id: String) -> Source {
    resource(
        move || Some((id.clone(), session.identity())),
        move |(id, _), _| {
            let state = session.with(|session| session.state.clone());
            bridge::request(async move {
                hongguo_api::detail::fetch_related_series(&id, &state.api_env()).await
            })
        },
    )
}

pub(super) fn content(source: Source, nav: Navigation, session: Option<SessionHandle>) -> View {
    let memberships = if let Some(session) = session {
        resource(
            move || {
                session
                    .with(|session| session.state.settings().account.is_some())
                    .then(|| (session.identity(), session.library()))
            },
            move |_, _| {
                let state = session.with(|session| session.state.clone());
                bridge::request(async move {
                    let env = state.api_env();
                    let (offline, online) = tokio::try_join!(
                        hongguo_api::reservation::fetch_reservations(false, &env),
                        hongguo_api::reservation::fetch_reservations(true, &env)
                    )?;
                    Ok(offline
                        .items
                        .into_iter()
                        .chain(online.items)
                        .map(|item| item.series_id)
                        .collect::<HashSet<_>>())
                })
            },
        )
    } else {
        resource(
            || Some(()),
            |_, _| std::future::ready(Ok(HashSet::<String>::new())),
        )
    };
    let known = computed(move || {
        let logged_out = session.is_some_and(|session| {
            session.identity();
            session.with(|session| session.state.settings().account.is_none())
        });
        if logged_out || memberships.loading() || memberships.error().is_some() {
            HashSet::new()
        } else {
            memberships.with(|items| items.cloned().unwrap_or_default())
        }
    });
    let (reservation, toggle) = if let Some(session) = session {
        let (state, toggle) = series_feed::reservation(session, nav);
        (state, stored(Box::new(toggle) as Box<dyn Fn(RankItem)>))
    } else {
        let state = series_feed::Reservation {
            changed: signal(HashMap::new()),
            pending: signal(HashSet::new()),
            feedback: signal(SharedString::default()),
        };
        (
            state,
            stored(Box::new(move |item: RankItem| {
                state.changed.update(|changed| {
                    changed.insert(item.series_id, !item.reserved);
                })
            }) as Box<dyn Fn(RankItem)>),
        )
    };
    view()
        .gap_4()
        .mt_2()
        .child(when(
            move || source.loading(),
            move || ui::status(move || tr("common.loading")),
        ))
        .child(when(
            move || source.error().is_some(),
            move || {
                crate::pages::error_state(
                    move || {
                        source
                            .error()
                            .map_or_else(SharedString::default, |error| error.text())
                    },
                    move || source.refetch(),
                )
            },
        ))
        .child(ui::status(reservation.feedback))
        .child(section(
            move || {
                source.with(|source| {
                    source
                        .map(|source| source.works.clone())
                        .unwrap_or_default()
                })
            },
            "detail.relatedWorks",
            nav,
            reservation,
            toggle,
            known,
        ))
        .child(section(
            move || {
                source.with(|source| {
                    source
                        .map(|source| source.guess.clone())
                        .unwrap_or_default()
                })
            },
            "detail.guessYouLike",
            nav,
            reservation,
            toggle,
            known,
        ))
}

fn section(
    items: impl IntoReactive<Vec<RelatedItem>>,
    title: &'static str,
    nav: Navigation,
    reservation: series_feed::Reservation,
    toggle: Stored<Box<dyn Fn(RankItem)>>,
    known: Memo<HashSet<String>>,
) -> View {
    let items = items.into_reactive();
    let items = computed(move || items.get());
    let width = signal(0.0_f32);
    view()
        .style_when(move || items.with(Vec::is_empty), |style| style.hidden())
        .gap_3()
        .child(
            text(move || tr(title))
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD),
        )
        .child(
            view()
                .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
                .grid()
                .grid_cols(move || (((width.get() + 12.0) / 140.0).floor() as u16).max(1))
                .gap_3()
                .child(each(
                    move || items.get(),
                    |item: &RelatedItem| item.series_id.clone(),
                    move |item, _| {
                        let reserved =
                            move || {
                                reservation.changed.with(|changed| {
                                    changed.get(&item.get().series_id).copied().unwrap_or_else(
                                        || known.with(|ids| ids.contains(&item.get().series_id)),
                                    )
                                })
                            };
                        view()
                            .gap_1()
                            .child(
                                ui::related_poster(move || {
                                    let item = item.get();
                                    let mut metadata = if item.episode_cnt > 0 {
                                        tr("detail.episodesCount")
                                            .replace("{count}", &item.episode_cnt.to_string())
                                    } else if item.tag == "即将上线" {
                                        item.tag.clone()
                                    } else {
                                        String::new()
                                    };
                                    if item.play_cnt > 0 {
                                        metadata.push_str(&format!(
                                            " · {}{}",
                                            crate::i18n::format_play_count(item.play_cnt),
                                            tr("detail.plays")
                                        ));
                                    }
                                    ui::RelatedPoster {
                                        title: item.title,
                                        cover: item.cover,
                                        tag: item.tag,
                                        score: item.score,
                                        metadata,
                                    }
                                })
                                .on_press(move || {
                                    nav.detail(crate::navigation::DetailPrefill::from_related(
                                        &item.get_untracked(),
                                    ))
                                }),
                            )
                            .child(when(
                                move || item.get().episode_cnt == 0,
                                move || {
                                    ui::reserve_action(reserved, move || {
                                        reservation
                                            .pending
                                            .with(|pending| pending.contains(&item.get().series_id))
                                    })
                                    .on_press(move || {
                                        toggle.with(|toggle| {
                                            toggle(RankItem {
                                                series_id: item.get_untracked().series_id,
                                                reserved: reserved(),
                                                ..Default::default()
                                            })
                                        })
                                    })
                                },
                            ))
                    },
                )),
        )
}
