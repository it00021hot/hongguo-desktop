use super::{ButtonVariant, controls::surface, radii};
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

#[derive(Clone, Default, PartialEq)]
pub(crate) struct RelatedPoster {
    pub title: String,
    pub cover: String,
    pub tag: String,
    pub score: f64,
    pub metadata: String,
}

pub(crate) fn related_poster(data: impl IntoReactive<RelatedPoster>) -> nsh_base::ButtonElement {
    let data = data.into_reactive();
    let data = computed(move || data.get());
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(data.get().title.into()))
        .w_full()
        .min_h(px(0.0))
        .p_0()
        .items_stretch()
        .justify_start()
        .hover(|style| {
            style
                .bg(move || theme().background.opacity(0.0))
                .opacity(0.95)
        })
        .active(|style| style.scale(1.0))
        .child(
            view()
                .relative()
                .w_full()
                .aspect_ratio(3.0 / 4.0)
                .overflow_hidden()
                .rounded(radii::control())
                .bg(move || theme().muted)
                .child(
                    image(move || ImageSrc::from(data.get().cover))
                        .absolute()
                        .inset_0()
                        .object_fit(nsh_core::ResizeMode::Cover),
                )
                .child(when(
                    move || !data.get().tag.is_empty(),
                    move || {
                        text(move || data.get().tag)
                            .absolute()
                            .top_1()
                            .left_1()
                            .px_1()
                            .py_0p5()
                            .rounded(px(4.0))
                            .text_size(px(10.0))
                            .line_height(px(10.0))
                            .font_weight(FontWeight::NORMAL)
                            .bg(move || {
                                theme()
                                    .color("video-surface")
                                    .expect("video surface token")
                                    .opacity(0.5)
                            })
                            .text_color(move || {
                                theme().color("video-controls").expect("video ink token")
                            })
                    },
                ))
                .child(when(
                    move || data.get().score > 0.0,
                    move || {
                        text(move || {
                            format!(
                                "{:.1}{}",
                                data.get().score,
                                crate::i18n::tr("detail.scoreUnit")
                            )
                        })
                        .absolute()
                        .bottom_1()
                        .right_1()
                        .px_1()
                        .py_0p5()
                        .rounded(px(4.0))
                        .text_size(px(10.0))
                        .line_height(px(10.0))
                        .font_weight(FontWeight::NORMAL)
                        .bg(move || {
                            theme()
                                .color("video-surface")
                                .expect("video surface token")
                                .opacity(0.6)
                        })
                        .text_color(move || theme().color("star").expect("star token"))
                    },
                )),
        )
        .child(
            text(move || data.get().title)
                .mt_1p5()
                .text_xs()
                .line_height(px(16.5))
                .font_weight(FontWeight::NORMAL)
                .number_of_lines(2),
        )
        .child(
            text(move || data.get().metadata)
                .mt_0p5()
                .text_size(px(11.0))
                .line_height(px(16.0))
                .font_weight(FontWeight::NORMAL)
                .text_color(move || theme().muted_foreground)
                .number_of_lines(1),
        )
}
