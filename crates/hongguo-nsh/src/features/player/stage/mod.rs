mod composer;
mod controls;
mod episodes;
mod landscape;
mod menu_model;
mod menus;
pub(crate) use menus::{MenuState, Panel, Season, SeasonState};
mod rail;
use crate::i18n::tr;
pub(crate) use composer::{ComposerEvents, composer};
pub(crate) use controls::AdvancedControl;
pub(crate) use controls::AdvancedState;
pub(crate) use controls::{ControlAction, ControlState, controls};

use nsh_core::prelude::*;
pub(crate) use rail::{RailAction, RailState, rail};

pub(crate) fn compact() -> bool {
    crate::features::player::presentation::player_narrow()
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
pub(crate) fn surface(
    video: impl IntoElement,
    info: impl IntoElement,
    rail: impl IntoElement,
    controls: impl IntoElement,
    status: impl IntoElement,
    show_binge_bar: Memo<bool>,
) -> View {
    let chrome = super::chrome::Chrome::current();
    let modes = crate::window_modes::WindowModes::current();
    let geometry = use_context::<crate::features::player::presentation::PlayerGeometry>();
    let overlay = crate::features::player::presentation::player_chrome(computed(move || {
        chrome.is_none_or(|chrome| chrome.shown().get())
    }))
    .right(move || px(geometry.map_or(0.0, |geometry| geometry.comment_width())))
    .child(
        view()
            .absolute()
            .left_3()
            .bottom(move || {
                px(if is_desktop() {
                    geometry.map_or(0.0, |geometry| geometry.info_bottom(show_binge_bar.get()))
                } else if compact() {
                    40.0
                } else if show_binge_bar.get() {
                    104.0
                } else {
                    80.0
                })
            })
            .right_20()
            .style_when(
                move || !is_desktop() && compact(),
                |style| {
                    style
                        .left_4()
                        .right_20()
                        .w(auto())
                        .bottom(move || px(if show_binge_bar.get() { 64.0 } else { 56.0 }))
                },
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
            .bottom(move || {
                px(if is_desktop() {
                    geometry.map_or(0.0, |geometry| geometry.info_bottom(show_binge_bar.get()))
                } else {
                    96.0
                })
            })
            .style_when(
                move || !is_desktop() && compact(),
                |style| style.right_2().bottom_24(),
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
    .child(controls)
    .z_index(20);
    let overlay = match chrome {
        Some(chrome) => chrome.bind_focus(overlay),
        None => overlay,
    };
    let surface = crate::features::player::presentation::player_stage()
        .on_layout(move |bounds: &Bounds<Points>| {
            if let Some(geometry) = geometry {
                geometry
                    .stage
                    .set((bounds.size.width.0, bounds.size.height.0));
            }
        })
        .relative()
        .overflow_hidden()
        .child(when(
            move || {
                modes.is_some_and(|modes| {
                    let state = modes.state();
                    !state.fullscreen
                        && (state.frameless || state.mode != crate::window_modes::Mode::Normal)
                })
            },
            crate::features::player::presentation::window_drag_bar,
        ))
        .child(
            view()
                .absolute()
                .inset_0()
                .right(move || px(geometry.map_or(0.0, |geometry| geometry.comment_width())))
                .style_when(
                    move || {
                        !is_desktop()
                            && geometry.is_some_and(|geometry| geometry.comments_open.get())
                    },
                    |style| {
                        style.bottom(move || {
                            px(geometry.map_or(0.0, |geometry| {
                                geometry.stage.get().1
                                    * super::presentation::comment_panel::MOBILE_HEIGHT_RATIO
                            }))
                        })
                    },
                )
                .on_layout(move |bounds: &Bounds<Points>| {
                    if let Some(geometry) = geometry {
                        geometry
                            .viewport
                            .set((bounds.size.width.0, bounds.size.height.0));
                    }
                })
                .child(video),
        )
        .child(when(
            move || is_desktop() && geometry.is_some(),
            move || {
                view()
                    .absolute()
                    .right_0()
                    .bottom_0()
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
        )
        .style_with(move |style| {
            let hide = is_desktop()
                && modes.is_some_and(|modes| modes.state().fullscreen)
                && chrome.is_some_and(|chrome| !chrome.shown().get());
            style.cursor(if hide { Cursor::None } else { Cursor::Auto })
        });
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
    pub(crate) heat: String,
    pub(crate) season: String,
    pub(crate) badge: String,
    pub(crate) highlight: Option<hongguo_common::model::Highlight>,
}

impl Info {
    fn clip_episode(&self) -> u32 {
        self.highlight.as_ref().map_or(0, |clip| {
            clip.episode_number.unwrap_or_else(|| {
                if clip.related.as_ref().is_some_and(|related| {
                    related.kind == hongguo_common::model::RelatedKind::Album
                }) && self.episode > 1
                {
                    self.episode
                } else {
                    0
                }
            })
        })
    }
}

impl From<Info> for crate::features::player::presentation::WindowCaption {
    fn from(info: Info) -> Self {
        let is_clip = info.highlight.is_some();
        Self {
            episode: if is_clip {
                info.clip_episode()
            } else {
                info.episode
            },
            title: info.title,
            total: if is_clip { 0 } else { info.total },
            highlight: info.highlight.map(|highlight| highlight.author),
        }
    }
}

pub(crate) fn info(
    model: impl IntoReactive<Info>,
    open_detail: impl Fn() + Copy + 'static,
) -> View {
    let model = model.into_reactive();
    let model = computed(move || model.get());
    view()
        .w_full()
        .gap_0p5()
        .font_weight(FontWeight::NORMAL)
        .style_when(
            move || {
                model.with(|info| {
                    info.highlight.is_none()
                        && (info.title.is_empty() || info.episode == 0 || info.total == 0)
                })
            },
            |style| style.hidden(),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(when(
                    move || model.with(|info| info.highlight.is_none() && !info.badge.is_empty()),
                    move || {
                        crate::features::player::presentation::player_campaign_badge(move || {
                            model.get().badge
                        })
                    },
                ))
                .child(when(
                    move || model.with(|info| info.highlight.is_none() && !info.season.is_empty()),
                    move || {
                        crate::features::player::presentation::player_badge(move || {
                            model.get().season
                        })
                    },
                ))
                .child(when(
                    move || model.with(|info| info.highlight.is_none()),
                    move || {
                        crate::features::player::presentation::player_title(move || {
                            model.get().title
                        })
                        .on_press(open_detail)
                    },
                ))
                .child(when(
                    move || {
                        model.with(|info| {
                            info.highlight
                                .as_ref()
                                .is_some_and(|clip| !clip.author.is_empty())
                        })
                    },
                    move || {
                        view().flex_row().items_center().gap_2().child(
                            crate::features::player::presentation::player_episode_label(
                                move || {
                                    model.with(|info| {
                                        info.highlight
                                            .as_ref()
                                            .map(|clip| clip.author.as_str())
                                            .filter(|author| !author.is_empty())
                                            .map(|author| format!("@{author}"))
                                            .unwrap_or_default()
                                    })
                                },
                            )
                            .number_of_lines(1),
                        )
                    },
                )),
        )
        .child(when(
            move || model.with(|info| info.highlight.is_none()),
            move || {
                crate::features::player::presentation::player_episode_label(move || {
                    model.with(|info| {
                        let episode =
                            tr("player.epShort").replace("{index}", &info.episode.to_string());
                        if !is_desktop() {
                            return episode;
                        }
                        let total =
                            tr("player.totalEpisodes").replace("{count}", &info.total.to_string());
                        format!("{episode} · {total}")
                    })
                })
                .number_of_lines(1)
            },
        ))
        .child(when(
            move || model.with(|info| info.highlight.is_some()),
            move || {
                crate::features::player::presentation::player_episode_label(move || {
                    model.with(|info| super::info::clip_caption(&info.title, info.clip_episode()))
                })
                .number_of_lines(2)
            },
        ))
}
