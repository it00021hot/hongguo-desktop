use super::Kind;
use crate::{i18n::tr, ui::loading::skeleton};
use nsh_core::prelude::*;

fn placeholder_group() -> View {
    view()
        .w_full()
        .min_h_0()
        .role(AccessibilityRole::Group)
        .label(move || tr("common.loading"))
}

pub(super) fn entries(kind: Kind) -> View {
    if !matches!(kind, Kind::Collections | Kind::Liked) {
        return rows(3, 149.0);
    }
    let columns = computed(move || super::presentation::personal_poster_columns(window().width.0));
    placeholder_group().child(dynamic(move || {
        view()
            .flex_row()
            .gap_4()
            .children((0..columns.get()).map(|_| {
                view()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(skeleton().w_full().aspect_ratio(3.0 / 4.0))
                    .child(skeleton().h_4().w(percentage(0.72)))
                    .child(skeleton().h_3().w(percentage(0.43)))
            }))
    }))
}

pub(super) fn rows(count: usize, height: f32) -> View {
    placeholder_group()
        .gap_2()
        .children((0..count).map(move |_| skeleton().w_full().h(px(height)).flex_shrink_0()))
}
