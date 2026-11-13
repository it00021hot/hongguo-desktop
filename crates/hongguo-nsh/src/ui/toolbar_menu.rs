use super::{Icon, icon, motion::FAST_MS, radii};
use crate::i18n::tr;
use nsh_base::{MenuEvent, theme};
use nsh_core::prelude::*;

pub(super) const THEMES: &[(&str, &str)] = &[
    ("auto", "settings.themeAuto"),
    ("light", "settings.themeLight"),
    ("dark", "settings.themeDark"),
];
pub(super) const LANGUAGES: &[(&str, &str)] =
    &[("zh-CN", "search.langZh"), ("en-US", "search.langEn")];

pub(super) fn change_language(language: crate::i18n::Language, value: &'static str) {
    if let Err(error) = language.select(value) {
        spawn(async move {
            let answer = nsh_base::alert_dialog()
                .title(tr("search.langLabel").to_string())
                .description(tr("settings.saveFailed").replace("{message}", &error))
                .cancel("close", tr("common.close"))
                .show();
            if let Err(error) = answer.await {
                log::error!("[NSH locale] could not present save failure: {error}");
            }
        });
    }
}

pub(super) fn menu(
    trigger: nsh_base::ButtonElement,
    choices: &'static [(&'static str, &'static str)],
    selected: impl Fn() -> String + 'static,
    select: impl Fn(&'static str) + 'static,
) -> nsh_base::MenuElement {
    let open = signal(false);
    let reference = element_ref();
    let selected = stored(Box::new(selected) as Box<dyn Fn() -> String>);
    let select = stored(Box::new(select) as Box<dyn Fn(&'static str)>);
    nsh_base::menu()
        .open(move || Some(open.get()))
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(4.0))
        .trigger(
            view().child(
                trigger
                    .element_ref(&reference)
                    .on_press(move || open.update(|value| *value = !*value)),
            ),
        )
        .on_open_change(move |event: &MenuEvent| {
            if let MenuEvent::OpenChange(value, _) = event {
                open.set(*value);
            }
        })
        .on_close(move || reference.focus())
        .popup_style(|style| {
            style
                .min_w(px(128.0))
                .p_1()
                .rounded(radii::control())
                .border_1()
                .border_color(move || theme().border)
                .bg(move || theme().popover)
                .text_color(move || theme().popover_foreground)
                .shadow_md()
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                .ease_out()
        })
        .popup_starting_style(|style| style.opacity(0.0))
        .popup_ending_style(|style| style.opacity(0.0))
        .child(dynamic(move || {
            view().children(choices.iter().map(move |&(value, label)| {
                nsh_base::menu_item()
                    .accessibility_label(move || Some(tr(label)))
                    .min_h(px(if is_desktop() { 32.0 } else { 48.0 }))
                    .px_2()
                    .py_1p5()
                    .rounded(px(4.0))
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .hover(|style| style.bg(move || theme().accent))
                    .focus(|style| style.bg(move || theme().accent))
                    .child(
                        text(move || tr(label))
                            .text_sm()
                            .font_weight(FontWeight::NORMAL)
                            .flex_1(),
                    )
                    .child(when(
                        move || selected.with(|selected| selected()) == value,
                        || icon(Icon::Check).size_3(),
                    ))
                    .on_select(move || select.with(|select| select(value)))
            }))
        }))
}
