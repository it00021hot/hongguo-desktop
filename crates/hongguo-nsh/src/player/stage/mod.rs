#[cfg(test)]
mod binge_tests;
mod composer;
mod controls;
mod episodes;
mod fixture;
mod landscape;
#[cfg(test)]
mod loading_tests;
mod menu_model;
mod menus;
pub(crate) use menus::{MenuState, Panel, Season, SeasonState};
#[cfg(test)]
mod menu_tests;
mod rail;
#[cfg(test)]
mod seek_tests;
use crate::{i18n::tr, ui};
pub(crate) use composer::{ComposerEvents, composer};
pub(crate) use controls::AdvancedControl;
pub(crate) use controls::AdvancedState;
pub(crate) use controls::{ControlAction, ControlState, controls};
pub(crate) use fixture::fixture;
pub(crate) use fixture::fixture_with_motion;
pub(crate) use fixture::fixture_with_window_modes;
use nsh_core::prelude::*;
pub(crate) use rail::{RailAction, RailState, rail};

pub(crate) fn compact() -> bool {
    ui::player_narrow()
        && crate::window_modes::WindowModes::current().is_none_or(|modes| {
            modes.state().mode == crate::window_modes::Mode::Normal && !modes.state().landscape
        })
}

const DESKTOP_COMPOSER_COLLAPSE_WIDTH: f32 = 1152.0;

fn desktop_composer_collapsed(width: f32) -> bool {
    is_desktop()
        && !compact()
        && !landscape::active()
        && width > 0.0
        && width < DESKTOP_COMPOSER_COLLAPSE_WIDTH
}

/// The production stage has one video plane and independent chrome slots.
/// A preview supplies a fixed frame in the same video slot; it never opens media.
pub(crate) fn surface(
    video: impl IntoElement,
    info: impl IntoElement,
    rail: impl IntoElement,
    controls: impl IntoElement,
    status: impl IntoElement,
) -> View {
    let chrome = super::chrome::Chrome::current();
    let modes = crate::window_modes::WindowModes::current();
    let geometry = use_context::<ui::PlayerGeometry>();
    let overlay = ui::player_chrome(computed(move || {
        chrome.is_none_or(|chrome| chrome.shown().get())
    }))
    .child(
        view()
            .absolute()
            .left_3()
            .bottom(move || px(if compact() { 40.0 } else { 112.0 }))
            .w(percentage(0.62))
            .style_when(
                move || !is_desktop() && compact(),
                |style| style.left_4().right_20().w(auto()).bottom_36(),
            )
            .style_when(
                move || {
                    modes.is_some_and(|modes| {
                        modes.state().mode == crate::window_modes::Mode::Mini
                            || modes.state().landscape
                    })
                },
                |style| style.hidden(),
            )
            .child(info),
    )
    .child(
        view()
            .absolute()
            .right_2()
            .bottom(px(96.0))
            .style_when(
                move || !is_desktop() && compact(),
                |style| style.right_3().bottom_36(),
            )
            .style_when(
                move || {
                    modes.is_some_and(|modes| {
                        modes.state().mode == crate::window_modes::Mode::Mini
                            || modes.state().landscape
                    })
                },
                |style| style.hidden(),
            )
            .child(rail),
    )
    .child(controls);
    let overlay = match chrome {
        Some(chrome) => chrome.bind_focus(overlay),
        None => overlay,
    };
    let surface = ui::player_stage()
        .on_layout(move |bounds: &Bounds<Points>| {
            if let Some(geometry) = geometry {
                geometry
                    .viewport
                    .set((bounds.size.width.0, bounds.size.height.0));
            }
        })
        .relative()
        .overflow_hidden()
        .test_id("player-stage")
        .child(view().absolute().inset_0().child(video))
        .child(when(
            move || is_desktop() && geometry.is_some(),
            move || {
                view()
                    .absolute()
                    .right_0()
                    .bottom(px(56.0))
                    .size(px(0.0))
                    .pointer_events(PointerEvents::None)
                    .element_ref(&geometry.expect("player geometry").comment_anchor)
            },
        ))
        .child(overlay)
        .child(
            view()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .pointer_events(PointerEvents::BoxNone)
                .child(status),
        );
    match chrome {
        Some(chrome) => surface
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| chrome.moved())
            .on_mouse_move(move |_: &nsh_core::MouseEvent| chrome.moved())
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| chrome.leave()),
        None => surface,
    }
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct Info {
    pub(crate) title: String,
    pub(crate) episode: u32,
    pub(crate) total: u32,
    pub(crate) intro: String,
    pub(crate) heat: String,
    pub(crate) season: String,
    pub(crate) badge: String,
}

impl From<Info> for ui::WindowCaption {
    fn from(info: Info) -> Self {
        Self {
            title: info.title,
            intro: info.intro,
            episode: info.episode,
            total: info.total,
        }
    }
}

pub(crate) fn info(model: impl IntoReactive<Info>, open: impl Fn() + 'static) -> View {
    let model = model.into_reactive();
    let model = computed(move || model.get());
    let expanded = signal(false);
    view()
        .w_full()
        .gap_0p5()
        .font_weight(FontWeight::NORMAL)
        .style_when(
            move || {
                model.with(|info| info.title.is_empty() || info.episode == 0 || info.total == 0)
            },
            |style| style.hidden(),
        )
        .child(when(
            move || !model.get().heat.is_empty(),
            move || ui::player_heat(move || model.get().heat),
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(when(
                    move || !model.get().badge.is_empty(),
                    move || ui::player_campaign_badge(move || model.get().badge),
                ))
                .child(when(
                    move || !model.get().season.is_empty(),
                    move || ui::player_badge(move || model.get().season),
                ))
                .child(
                    ui::player_title(move || {
                        if is_desktop() {
                            format!("@{}", model.get().title)
                        } else {
                            model.get().title
                        }
                    })
                    .on_press(open),
                ),
        )
        .child(
            text(move || {
                let info = model.get();
                format!(
                    "{} · {}",
                    tr("player.epShort").replace("{index}", &info.episode.to_string()),
                    tr("player.totalEpisodes").replace("{count}", &info.total.to_string())
                )
            })
            .text_xs()
            .style_when(move || !is_desktop(), |style| style.text_sm()),
        )
        .child(when(
            move || !model.get().intro.is_empty(),
            move || {
                view()
                    .flex_row()
                    .items_end()
                    .gap_2()
                    .max_w(move || percentage(if compact() { 1.0 } else { 0.36 }))
                    .style_when(move || compact() && is_desktop(), |style| style.invisible())
                    .child(
                        ui::player_intro(move || model.get().intro)
                            .flex_1()
                            .number_of_lines(move || if expanded.get() { 0_u32 } else { 2_u32 }),
                    )
                    .child(
                        ui::player_inline(move || {
                            tr(if expanded.get() {
                                "player.introCollapse"
                            } else {
                                "player.introExpand"
                            })
                        })
                        .on_press(move || expanded.update(|expanded| *expanded = !*expanded)),
                    )
            },
        ))
}
