use super::mobile_menu::MainMenu;
use nsh_base::theme;
use nsh_core::prelude::*;

fn control_height() -> Points {
    px(if os() == nsh_core::platform::Os::Ios {
        44.0
    } else {
        48.0
    })
}

pub(crate) fn mobile_catalog_header(channels: impl IntoElement) -> View {
    let mut header = view()
        .test_id("mobile-catalog-header")
        .h(control_height())
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .px_2()
        .gap_1()
        .bg(move || theme().background)
        .child(
            scroll()
                .test_id("mobile-catalog-channels")
                .horizontal()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(channels),
        );
    if let Some(menu) = MainMenu::current() {
        header = header.child(menu.trigger().flex_shrink_0());
    }
    header
}

pub(crate) fn mobile_channel_tab(
    label: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    nsh_base::button()
        .accessibility_label(move || label.get())
        .min_w(control_height())
        .h(control_height())
        .min_h(control_height())
        .flex_shrink_0()
        .px_3()
        .py_0()
        .rounded(px(0.0))
        .bg(move || theme().background.opacity(0.0))
        .border_b(px(2.0))
        .border_color(move || {
            if selected.get() {
                theme().primary
            } else {
                theme().background.opacity(0.0)
            }
        })
        .selected(selected)
        .hover(|style| style.bg(move || theme().accent))
        .active(|style| style.opacity(0.75))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                super::super::motion::FAST_MS
            }
        })
        .text_color(move || {
            if selected.get() {
                theme().foreground
            } else {
                theme().muted_foreground
            }
        })
        .child(
            text(label)
                .text_base()
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1),
        )
}
