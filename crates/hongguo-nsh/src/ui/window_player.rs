#[path = "window_player_button.rs"]
mod buttons;
#[path = "window_player_caption.rs"]
mod caption;
use super::{Icon, player_seek};
use crate::i18n::{format_duration, tr};
use caption::caption;
use nsh_base::theme;
use nsh_core::prelude::*;

const MINI_RATES: [f32; 4] = [1.0, 1.5, 2.0, 3.0];

#[derive(Clone, Default, PartialEq)]
pub(crate) struct WindowCaption {
    pub title: String,
    pub intro: String,
    pub episode: u32,
    pub total: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct WindowPlayerState {
    pub portrait: Memo<bool>,
    pub caption: Memo<WindowCaption>,
    pub paused: Memo<bool>,
    pub rate: Memo<f32>,
    pub muted: Memo<bool>,
    pub position: Memo<(f64, f64)>,
    pub busy: Memo<bool>,
    pub seek_pending: Memo<bool>,
    pub has_next: Memo<bool>,
    pub incognito: Memo<bool>,
    pub pinned: Memo<bool>,
    pub fill: Memo<bool>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum WindowPlayerAction {
    PlayPause,
    Previous,
    Next,
    SetRate(f32),
    Seek(f64),
    ToggleMute,
    ToggleIncognito,
    TogglePinned,
    ToggleFill,
    Exit,
}

pub(crate) fn window_drag_bar() -> View {
    view()
        .test_id("player-window-drag-bar")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h_6()
        .z_index(30)
        .window_drag(true)
}

pub(crate) fn window_player_controls(
    state: WindowPlayerState,
    action: impl Fn(WindowPlayerAction) + 'static,
) -> View {
    let action = stored(Box::new(action) as Box<dyn Fn(WindowPlayerAction)>);
    let send = move |intent| action.with(|action| action(intent));
    let dragging = signal(false);
    let seek = signal(state.position.get_untracked().0);
    effect(move || {
        let position = state.position.get();
        if state.busy.get() {
            dragging.set(false);
        }
        if !dragging.get() && !state.seek_pending.get() {
            seek.set(position.0);
        }
    });
    view()
        .test_id("player-window-controls")
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .z_index(20)
        .px_2()
        .pt_6()
        .pb(px(6.0))
        .gap_1()
        .bg_linear_to_t()
        .gradient_from(move || {
            theme()
                .color("video-surface")
                .expect("video surface")
                .opacity(0.75)
        })
        .gradient_to(move || {
            theme()
                .color("video-surface")
                .expect("video surface")
                .opacity(0.0)
        })
        .text_color(move || theme().color("video-controls").expect("video controls"))
        .child(when(
            move || !state.portrait.get(),
            move || caption(state.caption),
        ))
        .child(
            view().test_id("window-player-seek").child(
                player_seek(seek, move || state.position.get().1)
                    .accessibility_label(move || tr("common.progress"))
                    .disabled(move || state.busy.get() || state.position.get().1 <= 0.0)
                    .on_change(move |value: &nsh_base::SliderValue| {
                        dragging.set(true);
                        seek.set(value.start());
                    })
                    .on_change_end(move |value: &nsh_base::SliderValue| {
                        let maximum = state.position.get_untracked().1;
                        let target = value.start();
                        if !state.busy.get_untracked()
                            && maximum.is_finite()
                            && maximum > 0.0
                            && target.is_finite()
                        {
                            send(WindowPlayerAction::Seek(target.clamp(0.0, maximum)));
                        }
                        dragging.set(false);
                    }),
            ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_0p5()
                .child(
                    buttons::surface(move || {
                        tr(if state.paused.get() {
                            "player.play"
                        } else {
                            "player.pause"
                        })
                    })
                    .child(dynamic(move || {
                        buttons::glyph(if state.paused.get() {
                            Icon::Play
                        } else {
                            Icon::Pause
                        })
                    }))
                    .on_press(move || send(WindowPlayerAction::PlayPause)),
                )
                .child(
                    buttons::surface(move || tr("player.prevEpisode"))
                        .child(buttons::glyph(Icon::Previous))
                        .on_press(move || send(WindowPlayerAction::Previous)),
                )
                .child(
                    buttons::surface(move || tr("player.nextEpisode"))
                        .disabled(move || !state.has_next.get())
                        .child(buttons::glyph(Icon::Next))
                        .on_press(move || send(WindowPlayerAction::Next)),
                )
                .child(
                    text(move || {
                        let p = state.position.get();
                        format!("{} / {}", format_duration(p.0), format_duration(p.1))
                    })
                    .text_size(px(10.0))
                    .line_height(px(15.0))
                    .font_family("monospace")
                    .font_weight(FontWeight::NORMAL)
                    .truncate()
                    .text_color(move || {
                        theme()
                            .color("video-controls")
                            .expect("video controls")
                            .opacity(0.85)
                    }),
                )
                .child(
                    view()
                        .ml(auto())
                        .flex_row()
                        .items_center()
                        .gap_0p5()
                        .child(
                            buttons::surface(move || tr("player.playbackRate"))
                                .child(
                                    text(move || format!("{}x", state.rate.get()))
                                        .text_size(px(10.0))
                                        .line_height(px(15.0))
                                        .font_family("monospace")
                                        .font_weight(FontWeight::NORMAL),
                                )
                                .on_press(move || {
                                    send(WindowPlayerAction::SetRate(next_rate(
                                        state.rate.get_untracked(),
                                    )))
                                }),
                        )
                        .child(
                            buttons::surface(move || tr("player.mute"))
                                .child(dynamic(move || {
                                    buttons::glyph(if state.muted.get() {
                                        Icon::Muted
                                    } else {
                                        Icon::Volume
                                    })
                                }))
                                .on_press(move || send(WindowPlayerAction::ToggleMute)),
                        )
                        .child(
                            buttons::surface(move || tr("player.incognito"))
                                .selected(state.incognito)
                                .text_color(move || {
                                    theme()
                                        .color("video-controls")
                                        .expect("video controls")
                                        .opacity(if state.incognito.get() { 1.0 } else { 0.55 })
                                })
                                .child(buttons::glyph(Icon::Eye))
                                .on_press(move || send(WindowPlayerAction::ToggleIncognito)),
                        )
                        .child(when(
                            move || !state.portrait.get(),
                            move || {
                                buttons::surface(move || {
                                    tr(if state.pinned.get() {
                                        "player.unpin"
                                    } else {
                                        "player.pin"
                                    })
                                })
                                .selected(state.pinned)
                                .text_color(move || {
                                    theme().color("video-controls").expect("video controls")
                                })
                                .selected_style(|style| {
                                    style
                                        .bg(move || {
                                            theme()
                                                .color("video-surface")
                                                .expect("video surface")
                                                .opacity(0.0)
                                        })
                                        .border(px(0.0))
                                        .text_color(move || {
                                            theme().color("video-controls").expect("video controls")
                                        })
                                })
                                .child(dynamic(move || {
                                    buttons::glyph(if state.pinned.get() {
                                        Icon::PinFilled
                                    } else {
                                        Icon::Pin
                                    })
                                }))
                                .on_press(move || send(WindowPlayerAction::TogglePinned))
                            },
                        ))
                        .child(when(
                            move || state.portrait.get(),
                            move || {
                                buttons::surface(move || {
                                    tr(if state.fill.get() {
                                        "player.exitPortraitFill"
                                    } else {
                                        "player.portraitFill"
                                    })
                                })
                                .child(dynamic(move || {
                                    if state.fill.get() {
                                        buttons::special(buttons::COLLAPSE)
                                    } else {
                                        buttons::glyph(Icon::Fullscreen)
                                    }
                                }))
                                .on_press(move || send(WindowPlayerAction::ToggleFill))
                            },
                        ))
                        .child(
                            buttons::surface(move || {
                                tr(if state.portrait.get() {
                                    "player.exitPortrait"
                                } else {
                                    "player.exitMiniScreen"
                                })
                            })
                            .child(dynamic(move || {
                                buttons::special(if state.portrait.get() {
                                    buttons::LANDSCAPE
                                } else {
                                    buttons::EXPAND
                                })
                            }))
                            .on_press(move || send(WindowPlayerAction::Exit)),
                        ),
                ),
        )
}

fn next_rate(rate: f32) -> f32 {
    let next = MINI_RATES
        .iter()
        .position(|value| (*value - rate).abs() < 0.01)
        .map_or(0, |index| (index + 1) % MINI_RATES.len());
    MINI_RATES[next]
}
