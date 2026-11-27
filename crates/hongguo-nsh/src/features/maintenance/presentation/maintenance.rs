use crate::ui::ButtonVariant;
use crate::ui::controls::surface;
use crate::ui::radii;
use nsh_base::theme;
use nsh_core::prelude::*;

mod choices;
pub(crate) use choices::{maintenance_mode, maintenance_modes, maintenance_select};

#[derive(Clone, Copy)]
pub(crate) enum MaintenanceIcon {
    Merge,
    Quick,
    Compat,
    Delete,
    Folder,
    Storage,
}

pub(crate) fn maintenance_icon(glyph: MaintenanceIcon) -> Svg {
    let markup = match glyph {
        MaintenanceIcon::Merge => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/ui/icons/combine.svg"
        )),
        MaintenanceIcon::Quick => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui/icons/zap.svg"))
        }
        MaintenanceIcon::Compat => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/ui/icons/gauge.svg"
        )),
        MaintenanceIcon::Delete => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/ui/icons/trash-2.svg"
        )),
        MaintenanceIcon::Folder => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/ui/icons/folder-open.svg"
        )),
        MaintenanceIcon::Storage => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/ui/icons/hard-drive.svg"
        )),
    };
    svg(nsh_core::element::ImageSrc::svg_markup(markup))
        .size_4()
        .fill_none()
        .stroke_current()
        .stroke_width(px(2.0))
        .accessible(false)
}

pub(crate) fn maintenance_page() -> View {
    view().w_full().max_w(px(768.0)).self_center().p_6().gap_4()
}

pub(crate) fn maintenance_title(label: impl IntoReactive<SharedString>) -> Text {
    text(label).text_base().font_weight(FontWeight::SEMIBOLD)
}

pub(crate) fn maintenance_card(header: impl IntoElement) -> View {
    crate::features::settings::presentation::form_surface(header).shadow_sm()
}

pub(crate) fn maintenance_row() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .rounded(radii::control())
        .border_1()
        .border_color(move || theme().border)
        .px_3()
        .py_2()
}

pub(crate) fn maintenance_name(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .flex_1()
        .min_w(px(0.0))
        .truncate()
        .text_sm()
        .font_weight(FontWeight::NORMAL)
}

pub(crate) fn maintenance_bytes(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_xs()
        .font_weight(FontWeight::NORMAL)
        .tabular_nums()
        .text_color(move || theme().muted_foreground)
}

pub(crate) fn maintenance_icon_action(
    label: impl IntoReactive<SharedString>,
    glyph: MaintenanceIcon,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(label.get()))
        .min_h(crate::ui::controls::control_height)
        .size(crate::ui::controls::control_height)
        .p_0()
        .child(maintenance_icon(glyph))
}

pub(crate) fn maintenance_action(
    label: impl IntoReactive<SharedString>,
    glyph: MaintenanceIcon,
) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Primary)
        .flex_row()
        .gap_2()
        .min_h(crate::ui::controls::control_height)
        .h(crate::ui::controls::control_height)
        .py_0()
        .px_3()
        .child(maintenance_icon(glyph))
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
}

pub(crate) fn maintenance_destructive(
    label: impl IntoReactive<SharedString>,
) -> nsh_base::ButtonElement {
    maintenance_action(label, MaintenanceIcon::Delete)
        .px_2p5()
        .min_h(crate::ui::controls::control_height)
        .h(crate::ui::controls::control_height)
        .py_1p5()
        .bg(move || theme().destructive)
        .text_color(move || {
            theme()
                .color("destructive-foreground")
                .expect("destructive foreground")
        })
        .hover(|style| style.bg(move || theme().destructive.opacity(0.9)))
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MaintenanceTone {
    Secondary,
    Success,
    Destructive,
}

pub(crate) fn maintenance_badge(
    label: impl IntoReactive<SharedString>,
    tone: impl IntoReactive<MaintenanceTone>,
) -> View {
    let tone = tone.into_reactive();
    let tone = computed(move || tone.get());
    view()
        .px_2()
        .py_0p5()
        .flex_shrink_0()
        .rounded(radii::control())
        .border_1()
        .border_color(move || theme().border.opacity(0.0))
        .bg(move || match tone.get() {
            MaintenanceTone::Secondary => theme().secondary,
            MaintenanceTone::Success => theme()
                .color("success")
                .expect("success token")
                .opacity(0.15),
            MaintenanceTone::Destructive => theme().destructive,
        })
        .text_color(move || match tone.get() {
            MaintenanceTone::Secondary => theme().secondary_foreground,
            MaintenanceTone::Success => theme().color("success").expect("success token"),
            MaintenanceTone::Destructive => theme()
                .color("destructive-foreground")
                .expect("destructive foreground"),
        })
        .child(text(label).text_xs().font_weight(FontWeight::MEDIUM))
}

pub(crate) fn maintenance_progress(value: impl IntoReactive<f32>) -> nsh_base::ProgressElement {
    nsh_base::progress()
        .value(value)
        .track_style(|style| {
            style
                .h_1p5()
                .w_full()
                .rounded_full()
                .overflow_hidden()
                .bg(move || theme().secondary)
        })
        .indicator_style(|style| style.h_full().bg(move || theme().primary))
}

pub(crate) fn maintenance_alert(warning: impl IntoReactive<bool>) -> View {
    let warning = warning.into_reactive();
    let warning = computed(move || warning.get());
    view()
        .role(nsh_core::AccessibilityRole::Alert)
        .w_full()
        .rounded(move || theme().radius)
        .border_1()
        .px_4()
        .py_3()
        .gap_0p5()
        .border_color(move || {
            if warning.get() {
                theme()
                    .color("warning")
                    .expect("warning token")
                    .opacity(0.3)
            } else {
                theme().border
            }
        })
        .bg(move || {
            if warning.get() {
                theme()
                    .color("warning")
                    .expect("warning token")
                    .opacity(0.1)
            } else {
                theme().card
            }
        })
        .text_color(move || theme().card_foreground)
}
