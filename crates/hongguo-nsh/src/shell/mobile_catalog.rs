use super::mobile_menu::MainMenu;
use crate::{
    i18n::tr,
    navigation::{Navigation, Route},
    ui,
};
use nsh_base::theme;
use nsh_core::prelude::*;

fn control_height() -> Points {
    px(if is_desktop() {
        32.0
    } else if os() == nsh_core::platform::Os::Ios {
        44.0
    } else {
        48.0
    })
}

pub(crate) fn mobile_catalog_header(nav: Navigation, channels: impl IntoElement) -> View {
    let mut header = view()
        .h(control_height())
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .px_2()
        .bg(move || theme().background);
    header = header.style_when(
        move || super::chrome_hidden(nav) || super::PlayerOverlay::comments_open(),
        |style| style.hidden(),
    );
    if let Some(menu) = MainMenu::current() {
        header = header.child(menu.trigger().flex_shrink_0());
    }
    header
        .child(
            scroll().horizontal().flex_1().min_w_0().h_full().child(
                view()
                    .min_w(percentage(1.0))
                    .h_full()
                    .items_center()
                    .justify_center()
                    .child(channels),
            ),
        )
        .child(
            ui::controls::surface(ui::ButtonVariant::Quiet)
                .accessibility_label(move || tr("search.submit"))
                .size(control_height())
                .min_h(control_height())
                .p_0()
                .child(ui::icon(ui::Icon::Search).size_5())
                .on_press(move || nav.push(Route::Browse)),
        )
}

pub(crate) fn mobile_channel_tab(
    label: impl IntoReactive<SharedString>,
    selected: impl IntoReactive<bool>,
    pending: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let label = computed(move || label.get());
    let selected = selected.into_reactive();
    let selected = computed(move || selected.get());
    let pending = pending.into_reactive();
    let pending = computed(move || pending.get());
    nsh_base::button()
        .accessibility_label(move || label.get())
        .min_w(control_height())
        .h(control_height())
        .min_h(control_height())
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .px_3()
        .py_0()
        .relative()
        .rounded(px(0.0))
        .bg(move || theme().background.opacity(0.0))
        .selected(selected)
        .selected_style(ui::palette::selected_label)
        .hover(ui::palette::highlighted_label)
        .active(|style| style.opacity(0.75))
        .focus(|style| style.outline(px(2.0)).outline_color(move || theme().ring))
        .transition()
        .duration(move || {
            if reduce_motion() {
                0.0
            } else {
                crate::ui::motion::FAST_MS
            }
        })
        .text_color(move || {
            if selected.get() {
                theme().primary
            } else {
                theme().muted_foreground
            }
        })
        .child(
            text(label)
                .text_base()
                .style_when(is_desktop, |style| style.text_sm())
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1),
        )
        .child(
            view()
                .absolute()
                .bottom_1()
                .w_4()
                .h(px(2.0))
                .rounded_full()
                .bg(move || theme().primary)
                .style_when(pending, |style| style.animate_pulse())
                .style_when(reduce_motion, |style| style.animate_none())
                .style_when(move || !selected.get(), |style| style.hidden()),
        )
}
