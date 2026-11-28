use crate::features::player::presentation::player_popup;
use crate::i18n::tr;
use crate::ui::ButtonVariant;
use crate::ui::Icon;
use crate::ui::icon;
use crate::ui::palette;
use crate::ui::radii;
use nsh_base::SheetEvent;
use nsh_base::theme;
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
            player_popup::surface(style)
                .w_full()
                .max_h(move || px(window().height.0 * 0.8))
        })
        .child(dynamic(move || {
            view()
                .gap_4()
                .px(move || px(if is_desktop() { 24.0 } else { 16.0 }))
                .pt_4()
                .pb_safe_area()
                .pb_keyboard()
                .child(
                    view()
                        .relative()
                        .h(move || px(if is_desktop() { 24.0 } else { 48.0 }))
                        .justify_center()
                        .child(
                            text(move || tr("player.downloadToLocal"))
                                .text_base()
                                .line_height(px(24.0))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            surface(16.0)
                                .absolute()
                                .top(move || px(if is_desktop() { 4.0 } else { 0.0 }))
                                .right_0()
                                .size(move || px(if is_desktop() { 16.0 } else { 48.0 }))
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
                        .child(view().pb_4().child(content())),
                )
        }))
}

pub(crate) fn download_quick(
    label: impl IntoReactive<SharedString>,
    outline: bool,
) -> nsh_base::ButtonElement {
    let button = surface(32.0)
        .h_8()
        .px_3()
        .py_1()
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM));
    if outline {
        button
            .border_1()
            .border_color(move || nsh_base::theme().border)
            .bg(move || theme().secondary)
    } else {
        button
    }
}

pub(crate) fn download_episode(
    index: u32,
    selected: impl IntoReactive<bool>,
) -> nsh_base::ButtonElement {
    let selected = selected.into_reactive();
    surface(32.0)
        .h_8()
        .p_0()
        .border_1()
        .border_color(move || nsh_base::theme().border)
        .bg(move || theme().secondary)
        .selected(move || selected.get())
        .selected_style(|style| {
            palette::selected(style).border_color(palette::selection_background)
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
        .border_color(move || nsh_base::theme().border)
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
        .border_color(move || nsh_base::theme().border)
        .text_color(move || theme().foreground)
        .child(
            text(move || tr("download.selected").replace("{count}", &count.get().to_string()))
                .text_xs()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn download_range_error() -> Text {
    text(move || tr("download.rangeInvalid"))
        .text_xs()
        .text_color(move || nsh_base::theme().destructive)
}

pub(crate) fn download_cancel() -> nsh_base::ButtonElement {
    surface(36.0)
        .h(crate::ui::controls::control_height)
        .py_0()
        .border_1()
        .border_color(move || nsh_base::theme().border)
        .child(
            text(move || tr("common.cancel"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn download_submit() -> nsh_base::ButtonElement {
    surface(36.0)
        .bg(move || theme().primary)
        .text_color(move || theme().primary_foreground)
        .hover(|style| style.bg(move || theme().primary.opacity(0.9)))
        .disabled_style(palette::disabled)
        .flex_row()
        .gap_2()
        .child(icon(Icon::Download))
        .child(
            text(move || tr("download.submit"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn download_field(label: impl IntoReactive<SharedString>) -> nsh_base::Input {
    crate::ui::field(label)
        .text_color(move || theme().foreground)
        .placeholder_color(move || color(theme().muted_foreground))
        .border_color(move || nsh_base::theme().input)
        .caret_color(move || color(nsh_base::theme().ring))
        .selection_color(move || color(nsh_base::theme().primary.opacity(0.3)))
        .focus_visible(|style| {
            style
                .outline(px(2.0))
                .outline_color(move || nsh_base::theme().ring)
        })
}

fn surface(height: f32) -> nsh_base::ButtonElement {
    crate::ui::controls::surface_height(ButtonVariant::Quiet, height)
        .text_color(move || theme().foreground)
        .hover(|style| style.bg(move || theme().accent))
        .focus(|style| {
            style
                .outline(px(2.0))
                .outline_color(move || nsh_base::theme().ring)
        })
}
