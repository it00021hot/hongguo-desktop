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
use std::collections::HashSet;

type Source = Resource<RelatedSeries, bridge::Failure>;

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

pub(super) fn content(source: Source, nav: Navigation, session: SessionHandle) -> View {
    let memberships = resource(
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
    );
    let known = computed(move || {
        session.identity();
        let logged_out = session.with(|session| session.state.settings().account.is_none());
        if logged_out || memberships.loading() || memberships.error().is_some() {
            HashSet::new()
        } else {
            memberships.with(|items| items.cloned().unwrap_or_default())
        }
    });
    let (reservation, toggle) = series_feed::reservation(session, nav);
    let toggle = stored(Box::new(toggle) as Box<dyn Fn(RankItem)>);
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
