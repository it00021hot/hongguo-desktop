use crate::ui::loading::skeleton;
use crate::ui::radii;
use nsh_core::prelude::*;

const ROWS: usize = 8;

pub(crate) fn rank_loading() -> View {
    view()
        .flex_1()
        .min_h_0()
        .child(scroll().flex_1().child(loading_rows()))
}

pub(crate) fn loading_rows() -> View {
    view().gap_2().children((0..ROWS).map(|_| {
        view()
            .h(px(133.0))
            .flex_row()
            .items_center()
            .gap_3()
            .p_3()
            .rounded(radii::card())
            .border_1()
            .border_color(move || nsh_base::theme().border)
            .child(skeleton().w(px(80.0)).h(px(106.0)).flex_shrink_0())
            .child(
                view()
                    .flex_1()
                    .gap_2()
                    .child(skeleton().h_4().w(percentage(0.45)))
                    .child(skeleton().h_3().w(percentage(0.28)))
                    .child(skeleton().h_3().w(percentage(0.65))),
            )
    }))
}
