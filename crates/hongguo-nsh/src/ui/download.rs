use super::{ButtonVariant, Icon, controls::surface_height, icon, radii};
use crate::i18n::tr;
use nsh_base::{SheetEvent, theme};
use nsh_core::prelude::*;

pub(crate) fn download_sheet(
    open: Signal<bool>,
    content: impl Fn() -> View + 'static,
) -> nsh_base::SheetElement {
    nsh_base::sheet()
        .open(move || Some(open.get()))
        .side(Placement::Bottom)
        .on_open_change(move |event: &SheetEvent| {
            if let SheetEvent::OpenChange(next, _) = event {
                open.set(*next);
            }
        })
        .backdrop_style(|style| {
            style.bg(move || theme().color("dialog-backdrop").expect("backdrop token"))
        })
        .popup_style(|style| {
            style
                .w_full()
                .max_h(move || px(window().height.0 * 0.8))
                .border_t_1()
                .border_color(move || theme().border)
                .bg(move || theme().background)
                .text_color(move || theme().foreground)
        })
        .child(dynamic(move || {
            view()
                .test_id("player-download-sheet")
                .gap_4()
                .pb_safe_area()
                .pb_keyboard()
                .child(
                    view()
                        .relative()
                        .p_4()
                        .child(
                            text(move || tr("player.downloadToLocal"))
                                .text_base()
                                .line_height(px(24.0))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            surface_height(ButtonVariant::Quiet, 16.0)
                                .absolute()
                                .top_4()
                                .right_4()
                                .size_4()
                                .p_0()
                                .accessibility_label(move || Some(tr("common.close")))
                                .on_press(move || open.set(false))
                                .child(icon(Icon::Close)),
                        ),
                )
                .child(
                    scroll()
                        .min_h_0()
                        .max_h(move || px(window().height.0 * 0.8 - 72.0))
                        .child(content()),
                )
        }))
}

pub(crate) fn download_quick(
    label: impl IntoReactive<SharedString>,
    outline: bool,
) -> nsh_base::ButtonElement {
    let button = surface_height(ButtonVariant::Quiet, 32.0)
        .h_8()
        .px_3()
        .py_1()
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM));
    if outline {
        button
            .border_1()
            .border_color(move || theme().border)
            .bg(move || theme().background)
    } else {
        button
    }
}

pub(crate) fn download_episode(
    index: u32,
    selected: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let selected = selected.into_reactive();
    surface_height(ButtonVariant::Quiet, 32.0)
        .h_8()
        .p_0()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().background)
        .selected(move || selected.get())
        .selected_style(|style| {
            style
                .bg(move || theme().primary)
                .text_color(move || theme().primary_foreground)
                .border_color(move || theme().primary)
        })
        .accessibility_label(move || {
            Some(format!("{} {index}", tr("download.selectEpisodes")).into())
        })
        .child(
            text(index.to_string())
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn download_grid() -> Scroll {
    scroll()
        .max_h(px(256.0))
        .border_1()
        .border_color(move || theme().border)
        .rounded(radii::control())
        .p_2()
}

pub(crate) fn download_count(count: impl IntoReactive<usize>) -> View {
    let count = count.into_reactive();
    view()
        .px_2()
        .py_0p5()
        .rounded(radii::control())
        .bg(move || theme().secondary)
        .border_1()
        .border_color(move || theme().secondary)
        .text_color(move || theme().secondary_foreground)
        .child(
            text(move || tr("download.selected").replace("{count}", &count.get().to_string()))
                .text_xs()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn download_range_error() -> Text {
    text(move || tr("download.rangeInvalid"))
        .text_xs()
        .text_color(move || theme().destructive)
}

pub(crate) fn download_cancel() -> nsh_base::ButtonElement {
    super::button(move || tr("common.cancel"), ButtonVariant::Quiet)
        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
        .py_0()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().background)
}

pub(crate) fn download_submit() -> nsh_base::ButtonElement {
    super::controls::surface(ButtonVariant::Primary)
        .flex_row()
        .gap_2()
        .child(icon(Icon::Download))
        .child(
            text(move || tr("download.submit"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}
