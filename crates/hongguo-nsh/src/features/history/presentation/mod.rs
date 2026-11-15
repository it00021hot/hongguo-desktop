use crate::i18n::tr;
use crate::ui::Icon;
use crate::ui::icon;
use crate::ui::loading::skeleton;
use crate::ui::radii;
use nsh_base::theme;
use nsh_core::prelude::*;

const LOADING_ROWS: usize = 6;

pub(crate) fn history_loading() -> View {
    view()
        .role(AccessibilityRole::Group)
        .label(move || tr("common.loading"))
        .flex_1()
        .child(
            scroll()
                .flex_1()
                .child(view().p_4().gap_3().children((0..LOADING_ROWS).map(|_| {
                    skeleton()
                        .h(px(112.0))
                        .flex_shrink_0()
                        .rounded(radii::card())
                }))),
        )
}

pub(crate) fn history_empty() -> View {
    view()
        .w_full()
        .px(move || {
            px(if window().width.0 >= 768.0 {
                48.0
            } else {
                16.0
            })
        })
        .py(move || {
            px(if window().width.0 >= 768.0 {
                48.0
            } else {
                64.0
            })
        })
        .items_center()
        .child(
            view()
                .max_w(px(384.0))
                .items_center()
                .gap_2()
                .child(icon(Icon::History).size_8().opacity(0.4).mb_2())
                .child(
                    text(move || tr("history.empty"))
                        .text_sm()
                        .line_height(px(20.0))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(move || theme().muted_foreground),
                )
                .child(
                    text(move || tr("history.emptyHint"))
                        .text_xs()
                        .line_height(px(16.0))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(move || theme().muted_foreground)
                        .opacity(0.7),
                ),
        )
}
