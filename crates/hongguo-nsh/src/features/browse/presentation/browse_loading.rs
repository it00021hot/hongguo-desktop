use crate::features::browse::presentation::browse;
use crate::ui::radii;
use nsh_base::theme;
use nsh_core::prelude::*;

const SKELETON_COUNT: usize = 10;

pub(crate) fn browse_loading() -> View {
    let width = signal(0.0_f32);
    let columns = computed(move || browse::columns(window().width.0));
    view()
        .w_full()
        .on_layout(move |bounds: &Bounds<Points>| width.set(bounds.size.width.0))
        .child(
            virtual_list(
                || SKELETON_COUNT,
                |_| {
                    view()
                        .gap_2()
                        .child(
                            crate::ui::loading::skeleton()
                                .w_full()
                                .aspect_ratio(3.0 / 4.0)
                                .rounded(radii::card()),
                        )
                        .child(crate::ui::loading::skeleton().h_4().w(percentage(0.75)))
                        .child(crate::ui::loading::skeleton().h_3().w(percentage(0.5)))
                },
            )
            .viewport(nsh_core::ListViewport::Parent)
            .columns(columns)
            .column_gap(px(16.0))
            .row_gap(px(16.0))
            .estimated_item_size(move || {
                let columns = columns.get() as f32;
                px(((width.get() - (columns - 1.0) * 16.0) / columns).max(0.0) * 4.0 / 3.0 + 44.0)
            }),
        )
}

pub(crate) fn browse_loading_more() -> View {
    view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .py_2()
        .child(browse_spinner().accessible(false))
        .child(
            text(move || crate::i18n::tr("feed.loadingMore"))
                .text_sm()
                .font_weight(FontWeight::NORMAL),
        )
        .text_color(move || theme().muted_foreground)
}

pub(crate) fn browse_spinner() -> nsh_core::element::Svg {
    crate::ui::loading::spinner()
}

pub(crate) fn browse_empty(label: impl IntoReactive<SharedString>) -> View {
    view().py_16().items_center().child(
        text(label)
            .text_sm()
            .font_weight(FontWeight::NORMAL)
            .text_color(move || theme().muted_foreground),
    )
}
