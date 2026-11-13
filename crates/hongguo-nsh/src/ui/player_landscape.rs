use super::{Icon, WindowCaption, icon};
use crate::i18n::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct PlayerGeometry {
    pub source: Signal<(f64, f64)>,
    pub viewport: Signal<(f32, f32)>,
    pub comment_anchor: ElementRef,
    pub comment_trigger: ElementRef,
}

pub(crate) fn player_narrow() -> bool {
    let size = window();
    size.width.0 < 768.0 || (size.width.0 > size.height.0 && size.height.0 <= 500.0)
}

pub(crate) fn player_resize_mode(
    source: Signal<(f64, f64)>,
    mini: Memo<bool>,
) -> Memo<hongguo_nsh_video_api::ResizeMode> {
    computed(move || {
        let (width, height) = source.get();
        if mini.get() || (player_narrow() && width > 0.0 && height > width) {
            hongguo_nsh_video_api::ResizeMode::Cover
        } else {
            hongguo_nsh_video_api::ResizeMode::Contain
        }
    })
}

pub(crate) fn player_watch_fullscreen(
    geometry: PlayerGeometry,
    fullscreen: Memo<bool>,
    enter: impl Fn() + 'static,
) -> View {
    let enter = stored(Box::new(enter) as Box<dyn Fn()>);
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::BoxNone)
        .child(when(
            move || {
                let (width, height) = geometry.source.get();
                width > height && height > 0.0 && window().width.0 < 768.0 && !fullscreen.get()
            },
            move || {
                view()
                    .absolute()
                    .left_0()
                    .right_0()
                    .items_center()
                    .top(move || {
                        let (width, height) = geometry.viewport.get();
                        let (video_width, video_height) = geometry.source.get();
                        let rendered_height =
                            (f64::from(width) * video_height / video_width).min(f64::from(height));
                        px((f64::from(height) + rendered_height) as f32 * 0.5 + 10.0)
                    })
                    .child(
                        super::video::player_surface()
                            .flex_row()
                            .items_center()
                            .gap_1p5()
                            .rounded_full()
                            .px(px(14.0))
                            .py_2()
                            .min_h(move || {
                                px(if matches!(os(), nsh_core::platform::Os::Android) {
                                    48.0
                                } else {
                                    44.0
                                })
                            })
                            .bg(move || {
                                theme()
                                    .color("video-surface")
                                    .expect("video surface")
                                    .opacity(0.55)
                            })
                            .accessibility_label(move || Some(tr("player.watchFullscreen")))
                            .child(icon(Icon::MonitorPlay).size_4())
                            .child(text(move || tr("player.watchFullscreen")).text_sm())
                            .on_press(move || enter.with(|enter| enter())),
                    )
            },
        ))
}

pub(crate) fn player_landscape_header(
    caption: Memo<WindowCaption>,
    exit: impl Fn() + 'static,
) -> View {
    view()
        .test_id("player-landscape-header")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .z_index(30)
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .pt_2()
        .pb_6()
        .bg_linear_to_b()
        .gradient_from(move || {
            theme()
                .color("video-surface")
                .expect("video surface")
                .opacity(0.55)
        })
        .gradient_to(move || {
            theme()
                .color("video-surface")
                .expect("video surface")
                .opacity(0.0)
        })
        .child(
            super::video::player_surface()
                .size_11()
                .rounded_full()
                .accessibility_label(move || Some(tr("player.exitWatchFullscreen")))
                .child(icon(Icon::ChevronLeft).size_7())
                .on_press(exit),
        )
        .child(
            text(move || {
                let caption = caption.get();
                format!(
                    "{} {}",
                    caption.title,
                    tr("player.episodeNo").replace("{index}", &caption.episode.to_string())
                )
            })
            .flex_1()
            .min_w_0()
            .truncate()
            .text_sm()
            .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn player_landscape_row() -> View {
    view()
        .test_id("player-landscape-controls")
        .flex_row()
        .items_center()
        .gap_2()
        .mt_1()
}

pub(crate) fn player_landscape_label(
    label: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    super::video::player_surface()
        .flex_shrink_0()
        .px_1p5()
        .py_1()
        .rounded_md()
        .min_h(px(24.0))
        .h_6()
        .hit_slop(move || {
            EdgeInsets::all(px(if matches!(os(), nsh_core::platform::Os::Android) {
                12.0
            } else {
                10.0
            }))
        })
        .child(text(label).text_xs())
}

pub(crate) fn player_landscape_danmaku(selected: Signal<bool>) -> nsh_base::ButtonElement {
    super::video::player_surface()
        .size_8()
        .min_h(px(32.0))
        .p_0()
        .rounded_md()
        .hit_slop(move || {
            EdgeInsets::all(px(if matches!(os(), nsh_core::platform::Os::Android) {
                8.0
            } else {
                6.0
            }))
        })
        .accessibility_label(move || Some(tr("player.danmaku")))
        .selected(selected)
        .bg(move || {
            theme()
                .color("video-controls")
                .expect("video controls")
                .opacity(if selected.get() { 0.2 } else { 0.1 })
        })
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("video controls")
                .opacity(if selected.get() { 1.0 } else { 0.45 })
        })
        .selected_style(|style| {
            style.text_color(move || theme().color("video-controls").expect("video controls"))
        })
        .child(icon(Icon::Danmaku).size_4())
}

pub(crate) fn player_landscape_footer(footer: View, active: Memo<bool>) -> View {
    footer.style_when(
        move || active.get(),
        |style| {
            style
                .px_3()
                .pt_8()
                .pb_2p5()
                .gap_1()
                .bg_linear_to_t()
                .gradient_from(move || {
                    theme()
                        .color("video-surface")
                        .expect("video surface")
                        .opacity(0.7)
                })
                .gradient_to(move || {
                    theme()
                        .color("video-surface")
                        .expect("video surface")
                        .opacity(0.0)
                })
        },
    )
}

pub(crate) fn player_seek_rail(slider: nsh_base::SliderElement, landscape: Memo<bool>) -> View {
    view()
        .relative()
        .w_full()
        .style_when(move || landscape.get(), |style| style.h_4())
        .child(
            view()
                .test_id("player-seek-rail")
                .w_full()
                .focusable(false)
                .accessible(false)
                // The slider responder handles seeking; consume the same native tap here.
                .on_press(|| {})
                .style_when(
                    move || landscape.get() && !is_desktop(),
                    |style| style.absolute().left_0().right_0().top(px(-16.0)),
                )
                .child(slider),
        )
}
