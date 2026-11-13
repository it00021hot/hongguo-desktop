use super::{DetailState, creators, hero, related};
use crate::{i18n::tr, navigation::Route, pages::error_state, ui};
use nsh_core::prelude::*;

pub(super) fn render(state: DetailState) -> View {
    let DetailState {
        nav,
        result,
        metadata,
        social,
        resume,
        related,
        session,
        id,
        prefill,
        reservation,
    } = state;
    let prefill = stored(prefill);
    let upcoming = computed(move || {
        prefill.with(|seed| seed.is_some())
            && !result.loading()
            && result.with(|series| series.is_none())
            && result.error().is_some()
    });
    let displayed = computed(move || {
        result.get().or_else(|| {
            upcoming.get().then(|| {
                prefill.with(|seed| {
                    seed.as_ref()
                        .expect("upcoming has a source snapshot")
                        .series()
                })
            })
        })
    });
    let expanded = signal(false);
    let intro = computed(move || {
        metadata.with(|meta| meta.map(|meta| meta.intro.clone()).unwrap_or_default())
    });
    let tab = signal("episodes");
    let selected_episode = computed(move || {
        if resume.error().is_some() {
            return None;
        }
        let last = resume.get().flatten();
        result.with(|series| {
            series.and_then(|series| crate::player::start_episode(series, last).ok())
        })
    });
    let header = move || {
        ui::detail_section()
        .pt(move || px(if window().width.0 >= 768.0 { 24.0 } else { 16.0 }))
        .child(ui::detail_action(move || tr("detail.back"), ui::Icon::Back, false)
            .border(px(0.0)).text_color(move || nsh_base::theme().muted_foreground).self_start().px_0()
            .on_press(move || nav.back()))
        .child(hero::hero(displayed, metadata, upcoming).mt_4())
        .child(view().mt_5().style_when(move ||intro.get().is_empty(),|style|style.hidden())
            .child(text(move || tr("detail.plotIntro")).text_sm().line_height(px(20.0)).font_weight(FontWeight::SEMIBOLD))
            .child(text(move ||intro.get()).mt_1p5().text_sm().line_height(px(22.75))
                .font_weight(FontWeight::NORMAL).text_color(move || nsh_base::theme().muted_foreground)
                .number_of_lines(move || if expanded.get() { 0_u32 } else { 2_u32 }))
            .child(when(move ||!expanded.get()&&intro.with(|intro|intro.chars().count()>48),move ||ui::detail_inline(move || tr("player.introExpand")).mt_1()
                .on_press(move || expanded.set(true)))))
        .child(creators::content(metadata, move |id| nav.push(Route::Creator(id))))
        .child(when(upcoming, move || view().mt_5().flex_row().flex_wrap().items_center().gap_2()
            .child(view().flex_row().items_center().gap_1p5().text_color(move ||nsh_base::theme().color("hot").expect("hot token"))
                .child(ui::icon(ui::Icon::Hourglass).accessible(false))
                .child(text(move ||tr("player.comingSoon")).text_sm().font_weight(FontWeight::MEDIUM)))
            .child(ui::reserve_action(reservation.selected,reservation.busy).on_press(move ||reservation.submit()))))
        .child(when(move ||upcoming.get()&&!reservation.message.get().is_empty(),move ||view().mt_2().gap_2()
            .child(text(reservation.message).text_sm().text_color(move ||nsh_base::theme().muted_foreground))
            .child(when(reservation.failed,move ||ui::detail_inline(move ||tr("common.retry")).on_press(move ||reservation.retry())))))
        .child(view().mt_5().flex_row().gap_2().style_when(upcoming,|style|style.hidden())
            .child(ui::detail_action(move || selected_episode.get().filter(|index|*index>1).map_or_else(||tr("detail.playFirst"),|index|tr("detail.continueEpisode").replace("{index}",&index.to_string()).into()), ui::Icon::Play, true)
                .style_when(move ||window().width.0<768.0,|style|style.w_full())
                .disabled(move || resume.loading() ||selected_episode.get().is_none())
                .on_press(move || { if let Some((id,index)) = result.with(|series| series.and_then(|series| selected_episode.get_untracked().map(|episode| (series.series_id.clone(),episode)))) { nav.push(Route::Player(id,index)); } }))
            .child(ui::detail_action(move || tr(if social.collected.get() { "detail.collected" } else { "detail.collect" }), ui::Icon::Collections, false)
                .style_when(move ||window().width.0<768.0,|style|style.hidden()).disabled(social.busy).selected(social.collected).on_press(move || social.collect()))
            .child(ui::detail_action(move || tr("player.interact.like"), ui::Icon::Liked, false)
                .style_when(move ||window().width.0<768.0,|style|style.hidden()).disabled(social.busy).selected(social.liked).on_press(move || social.like()))
            .child(when(
                move || metadata.with(|meta| meta.is_some_and(|meta| meta.series_status == 0)),
                move || {
                    ui::detail_action(
                        move || tr(if social.followed.get() { "follow.following" } else { "follow.follow" }),
                        ui::Icon::Follows,
                        false,
                    )
                    .style_when(move || window().width.0 < 768.0, |style| style.hidden())
                    .disabled(social.busy)
                    .selected(social.followed)
                    .on_press(move || social.follow())
                },
            )))
        .child(when(move ||!social.message.get().is_empty(),move ||error_state(social.message,move ||social.retry())))
        .child(when(move ||resume.error().is_some(),move ||error_state(move ||resume.error().map_or_else(SharedString::default,|error|error.text()),move ||resume.refetch())))
        .child(when(move ||metadata.with(|meta|meta.is_some_and(|meta|!meta.record_number.is_empty())),move ||text(move ||metadata.with(|meta|meta.map(|meta|meta.record_number.clone()).unwrap_or_default())).mt_4().text_xs().font_weight(FontWeight::NORMAL).text_color(move ||nsh_base::theme().muted_foreground.opacity(0.7))))
        .child(when(move || result.loading(), move || ui::status(move || tr("common.resolving"))))
        .child(when(move || result.error().is_some()&&!upcoming.get(), move || error_state(move || result.error().map_or_else(SharedString::default, |error| error.text()), move || result.refetch())))
        .child(ui::tabs().mt_8()
            .child(ui::tab(move || format!("{} {}", tr("detail.tabEpisodes"), result.with(|series| series.map_or(0, |series| series.episodes.len())))).selected(move || tab.get() == "episodes").on_press(move || tab.set("episodes")))
            .child(ui::tab(move || tr("detail.tabComments")).selected(move || tab.get() == "comments").on_press(move || tab.set("comments")))
            .child(ui::tab(move || tr("detail.tabRecommend")).selected(move || tab.get() == "recommend").on_press(move || tab.set("recommend"))))
    };
    view()
        .relative()
        .flex_1()
        .min_h_0()
        .test_id("detail-scroll")
        .child(when(
            move || tab.get() != "comments",
            move || {
                ui::form(
                    header()
                        .child(when(
                            move || tab.get() == "episodes",
                            move || {
                                view()
                                    .mt_2()
                                    .grid()
                                    .grid_cols(move || {
                                        if window().width.0 < 768.0 {
                                            6_u16
                                        } else if window().width.0 >= 1536.0 {
                                            5
                                        } else if window().width.0 >= 1024.0 {
                                            4
                                        } else {
                                            3
                                        }
                                    })
                                    .gap(move || {
                                        px(if window().width.0 < 768.0 { 8.0 } else { 12.0 })
                                    })
                                    .child(when(upcoming, move || {
                                        view()
                                            .py(move || {
                                                px(if window().width.0 >= 768.0 {
                                                    48.0
                                                } else {
                                                    40.0
                                                })
                                            })
                                            .w_full()
                                            .items_center()
                                            .col_span_full()
                                            .child(
                                                text(move || tr("series.noEpisodes"))
                                                    .text_sm()
                                                    .font_weight(FontWeight::NORMAL)
                                                    .text_color(move || {
                                                        nsh_base::theme().muted_foreground
                                                    }),
                                            )
                                    }))
                                    .child(each(
                                        move || {
                                            result.with(|series| {
                                                series
                                                    .map(|series| series.episodes.clone())
                                                    .unwrap_or_default()
                                            })
                                        },
                                        |episode: &hongguo_common::model::Episode| {
                                            episode.vid_index
                                        },
                                        move |episode, _| {
                                            ui::episode_card(
                                                move || episode.get().vid_index,
                                                move || episode.get().duration,
                                            )
                                            .selected(move || {
                                                Some(episode.get().vid_index)
                                                    == selected_episode.get()
                                            })
                                            .on_press(
                                                move || {
                                                    if let Some(id) = result.with(|series| {
                                                        series
                                                            .map(|series| series.series_id.clone())
                                                    }) {
                                                        nav.push(Route::Player(
                                                            id,
                                                            episode.get_untracked().vid_index,
                                                        ));
                                                    }
                                                },
                                            )
                                        },
                                    ))
                            },
                        ))
                        .child(when(
                            move || tab.get() == "recommend",
                            move || related::content(related, nav, session),
                        ))
                        .pb(move || {
                            px(if window().width.0 >= 768.0 {
                                24.0
                            } else {
                                144.0
                            })
                        }),
                )
            },
        ))
        .child(when(
            move || tab.get() == "comments",
            move || {
                view().flex_1().min_h_0().child(match session {
                    Some(session) => {
                        crate::player::reviews_with_header(session, nav, id.get(), header())
                            .into_element()
                    }
                    None => crate::player::reviews_preview_with_header(header()).into_element(),
                })
            },
        ))
        .child(when(
            move || {
                window().width.0 < 768.0
                    && !upcoming.get()
                    && displayed.with(|series| series.is_some())
            },
            move || {
                ui::detail_mobile_actions(
                    social.collected,
                    social.liked,
                    social.busy,
                    move || social.collect(),
                    move || social.like(),
                )
            },
        ))
}
