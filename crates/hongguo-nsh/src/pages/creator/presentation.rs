use super::{Profile, Works};
use crate::{i18n::tr, navigation::Navigation, ui};
use nsh_core::prelude::*;

pub(super) fn content(
    nav: Navigation,
    profile: Profile,
    works: Works,
    open: impl Fn(String) + 'static,
    prefetch: impl Fn(String) + 'static,
    load_more: impl Fn() + 'static,
) -> View {
    let data = computed(move || {
        profile.with(|profile| {
            profile.map(|profile| ui::CreatorProfileData {
                name: profile.user_name.clone(),
                avatar: crate::image_url::cdn_image_url(&profile.avatar),
                description: profile.description.clone(),
            })
        })
    });
    let cards = computed(move || {
        works.with(|page| {
            page.map(|page| {
                page.works
                    .iter()
                    .map(|item| ui::CreatorCard {
                        id: item.series_id.clone(),
                        title: item.title.clone(),
                        cover: crate::image_url::cdn_image_url(&item.cover),
                        episodes: item.episode_cnt,
                        hot: item.hot,
                        plays: item.play_cnt,
                    })
                    .collect()
            })
            .unwrap_or_default()
        })
    });
    let pending = computed(move || works.loading() && works.with(|page| page.is_none()));
    let open = stored(open);
    let prefetch = stored(prefetch);
    let load_more = stored(load_more);
    ui::form(
        ui::creator_page()
            .child(ui::creator_back().on_press(move || nav.back()))
            .child(when(
                move || profile.loading() && data.with(Option::is_none),
                ui::creator_profile_skeleton,
            ))
            .child(when(
                move || data.with(Option::is_some),
                move || ui::creator_profile(computed(move || data.get().unwrap_or_default())),
            ))
            .child(when(
                move || works.with(|page| page.is_some()),
                move || {
                    ui::creator_works_heading(move || {
                        works.with(|page| page.map_or(0, |page| page.total))
                    })
                },
            ))
            .child(when(move || pending.get(), ui::creator_skeleton_grid))
            .child(when(
                move || works.error().is_some(),
                move || {
                    view()
                        .mt_3()
                        .child(
                            text(move || tr("creator.loadFailed"))
                                .text_sm()
                                .text_color(move || nsh_base::theme().destructive),
                        )
                        .child(crate::pages::error_state(
                            move || {
                                works
                                    .error()
                                    .map_or_else(SharedString::default, |error| error.text())
                            },
                            move || works.refetch(),
                        ))
                },
            ))
            .child(when(
                move || !pending.get() && works.error().is_none() && cards.with(Vec::is_empty),
                ui::creator_empty,
            ))
            .child(when(
                move || !pending.get() && works.error().is_none() && !cards.with(Vec::is_empty),
                move || {
                    ui::creator_grid(
                        cards,
                        move |id| open.with(|open| open(id)),
                        move |id| prefetch.with(|prefetch| prefetch(id)),
                        move || load_more.with(|load| load()),
                    )
                },
            ))
            .child(when(
                move || works.loading() && !pending.get(),
                ui::creator_loading_more,
            )),
    )
}
