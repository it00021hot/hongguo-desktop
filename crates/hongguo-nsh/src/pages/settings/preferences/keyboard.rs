use super::model::Draft;
use crate::{
    i18n::tr,
    navigation::AccountPrompt,
    shortcuts::{Action, Shortcuts},
    ui,
};
use nsh_core::prelude::*;

pub(super) fn shortcuts(active: Memo<bool>) -> View {
    let keys = Shortcuts::current().expect("app provides shortcut owner");
    let account = use_context::<AccountPrompt>();
    effect(move || {
        if !active.get() || account.is_some_and(|account| account.open()) {
            keys.cancel_capture();
        }
    });
    cleanup(move || keys.cancel_capture());
    ui::form_surface(
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                text(move || tr("settings.shortcuts"))
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(view().flex_1())
            .child(
                ui::form_action(move || tr("settings.resetKeys"), ui::ButtonVariant::Quiet)
                    .on_press(move || keys.reset()),
            ),
    )
    .child(
        view()
            .gap_1()
            .child(ui::form_description(move || tr("settings.shortcutsHint")).mb_2())
            .children(Action::ALL.into_iter().map(move |action| {
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .child(
                        text(move || tr(action.label_key()))
                            .text_sm()
                            .line_height(px(20.0)),
                    )
                    .child(
                        ui::settings_key(
                            move || {
                                if keys.capturing() == Some(action) {
                                    tr("settings.keyCapturing")
                                } else {
                                    keys.label(action)
                                }
                            },
                            move || keys.capturing() == Some(action),
                            ui::KeyBindingKind::Player,
                        )
                        .accessibility_label(move || Some(tr(action.label_key())))
                        .on_press(move || keys.capture(action)),
                    )
            }))
            .child(ui::status(move || keys.notice())),
    )
}

pub(super) fn boss(draft: Signal<Draft>, busy: Signal<bool>) -> impl IntoElement {
    let keys = Shortcuts::current().expect("app provides shortcut owner");
    when(is_desktop, move || {
        ui::form_surface(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(ui::icon(ui::Icon::Globe))
                .child(
                    text(move || tr("settings.bossKeyCardTitle"))
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        )
        .child(
            view()
                .gap_2()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            text(move || tr("settings.bossKey"))
                                .text_sm()
                                .line_height(px(20.0)),
                        )
                        .child(
                            ui::settings_key(
                                move || {
                                    if keys.capturing_boss() {
                                        tr("settings.bossKeyCapturing")
                                    } else {
                                        let key = draft.with(|draft| draft.boss_key.clone());
                                        if key.is_empty() {
                                            tr("settings.bossKeyOff")
                                        } else {
                                            key.split('+')
                                                .map(|part| {
                                                    if part == "CommandOrControl" {
                                                        "Ctrl/Cmd"
                                                    } else {
                                                        part
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                                .join(" + ")
                                                .into()
                                        }
                                    }
                                },
                                move || keys.capturing_boss(),
                                ui::KeyBindingKind::Boss,
                            )
                            .accessibility_label(move || Some(tr("settings.bossKey")))
                            .disabled(busy)
                            .on_press(move || {
                                keys.capture_boss(move |spec| {
                                    draft.update(|draft| draft.boss_key = spec)
                                })
                            }),
                        ),
                )
                .child(when(
                    move || {
                        !keys
                            .conflicts(&draft.with(|draft| draft.boss_key.clone()))
                            .is_empty()
                    },
                    move || {
                        text(move || {
                            tr("settings.bossKeyConflict").replace(
                                "{actions}",
                                &keys
                                    .conflicts(&draft.with(|draft| draft.boss_key.clone()))
                                    .into_iter()
                                    .map(|action| keys.label(action).to_string())
                                    .collect::<Vec<_>>()
                                    .join("、"),
                            )
                        })
                        .text_xs()
                        .line_height(px(16.0))
                        .text_color(move || nsh_base::theme().destructive)
                    },
                ))
                .child(ui::form_description(move || tr("settings.bossKeyDesc")))
                .child(ui::form_description(move || {
                    tr("settings.bossKeyGlobalHint")
                })),
        )
    })
}
