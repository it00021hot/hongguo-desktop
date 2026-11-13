use super::{motion::FAST_MS, radii};
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

pub(crate) struct SeriesRowParts {
    pub(crate) leading: Option<AnyElement>,
    pub(crate) content: AnyElement,
    pub(crate) actions: Option<AnyElement>,
}

pub(crate) fn series_row(
    title: impl IntoReactive<SharedString>,
    cover: impl IntoReactive<String>,
    parts: SeriesRowParts,
    open: impl Fn() + 'static,
) -> View {
    let title = title.into_reactive();
    let cover = cover.into_reactive();
    view()
        .w_full()
        .min_w(px(0.0))
        .flex_row()
        .items_center()
        .gap_4()
        .p_3()
        .rounded(radii::card())
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .role(nsh_core::AccessibilityRole::Button)
        .label(move || title.get())
        .focusable(true)
        .hover(|style| {
            style
                .border_color(move || theme().foreground.opacity(0.3))
                .shadow_md()
        })
        .focus(|style| style.border_color(move || theme().foreground.opacity(0.3)))
        .transition()
        .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
        .child(parts.leading)
        .child(
            view()
                .relative()
                .w_20()
                .flex_shrink_0()
                .aspect_ratio(3.0 / 4.0)
                .rounded(move || theme().radius)
                .overflow_hidden()
                .bg(move || theme().muted)
                .child(
                    image(move || ImageSrc::from(cover.get()))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .accessible(false)
                        .object_fit(nsh_core::ResizeMode::Cover),
                ),
        )
        .child(view().flex_1().min_w(px(0.0)).child(parts.content))
        .child(parts.actions)
        .on_press(open)
}
