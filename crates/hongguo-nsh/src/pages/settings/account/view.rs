use super::{Action, State, model};
use crate::{i18n::tr, ui};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(super) fn form(state: State, dispatch: impl Fn(Action) + Copy + 'static) -> View {
    let code_ref = element_ref();
    let focused_version = stored(state.code_focus.get_untracked());
    effect(move || {
        let next = state.code_focus.get();
        if focused_version.replace(next) != next {
            code_ref.focus();
        }
    });
    view()
        .gap_3()
        .child(when(
            move || state.mfa.get().is_none(),
            move || {
                view()
                    .gap_7()
                    .child(
                        view()
                            .gap_3()
                            .child(label("settings.loginMobilePlaceholder"))
                            .child(
                                ui::login_input(move || tr("settings.loginMobilePlaceholder"))
                                    .test_id("login-mobile")
                                    .auto_focus(true)
                                    .keyboard_type(nsh_core::KeyboardType::Phone)
                                    .auto_fill(nsh_core::AutoFill::Tel)
                                    .max_length(model::MOBILE_LENGTH as u32)
                                    .return_key_type(nsh_core::ReturnKeyType::Next)
                                    .disabled(state.busy)
                                    .value(state.mobile)
                                    .on_change(move |input: &str| {
                                        state.mobile.set(model::digits(input, model::MOBILE_LENGTH))
                                    })
                                    .on_submit(move |_: &str| code_ref.focus()),
                            ),
                    )
                    .child(
                        view()
                            .gap_3()
                            .child(label("settings.loginCodePlaceholder"))
                            .child(
                                view()
                                    .flex_row()
                                    .gap_2()
                                    .child(
                                        ui::login_input(move || {
                                            tr("settings.loginCodePlaceholder")
                                        })
                                        .element_ref(&code_ref)
                                        .test_id("login-code")
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .keyboard_type(nsh_core::KeyboardType::Number)
                                        .auto_fill(nsh_core::AutoFill::OneTimeCode)
                                        .max_length(model::CODE_LENGTH as u32)
                                        .return_key_type(nsh_core::ReturnKeyType::Done)
                                        .disabled(state.busy)
                                        .value(state.code)
                                        .on_change(move |input: &str| {
                                            state.code.set(model::digits(input, model::CODE_LENGTH))
                                        })
                                        .on_submit(
                                            move |_: &str| {
                                                if state.can_login() {
                                                    dispatch(Action::Login);
                                                }
                                            },
                                        ),
                                    )
                                    .child(
                                        ui::login_send(move || {
                                            let remaining = state.countdown();
                                            if remaining > 0 {
                                                tr("settings.loginResend")
                                                    .replace("{s}", &remaining.to_string())
                                                    .into()
                                            } else {
                                                tr(if state.busy.get() {
                                                    "settings.loginSending"
                                                } else {
                                                    "settings.loginSendCode"
                                                })
                                            }
                                        })
                                        .disabled(move || {
                                            state.busy.get()
                                                || state.countdown() > 0
                                                || !model::valid_mobile(&state.mobile.get())
                                        })
                                        .on_press(move || dispatch(Action::Send)),
                                    ),
                            ),
                    )
                    .child(
                        ui::button(
                            move || {
                                tr(if state.busy.get() {
                                    "settings.loginBusy"
                                } else {
                                    "settings.loginAction"
                                })
                            },
                            ui::ButtonVariant::Primary,
                        )
                        .h(move || px(if is_desktop() { 36.0 } else { 48.0 }))
                        .py_0()
                        .disabled(move || !state.can_login())
                        .on_press(move || dispatch(Action::Login)),
                    )
                    .child(
                        text(move || tr("settings.loginRiskNote"))
                            .text_xs()
                            .line_height(px(16.0))
                            .text_color(move || theme().muted_foreground),
                    )
            },
        ))
        .child(when(
            move || state.mfa.get().is_some(),
            move || {
                view()
                    .gap_3()
                    .child(
                        text(move || tr("settings.loginMfaWaiting"))
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(
                        text(move || masked(&state.mobile.get()))
                            .text_sm()
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_start()
                            .gap_2()
                            .rounded(move || theme().radius)
                            .p_3()
                            .bg(move || theme().muted)
                            .child(
                                ui::icon(ui::Icon::ShieldCheck)
                                    .size_4()
                                    .text_color(move || theme().primary),
                            )
                            .child(
                                text(move || state.mfa.get().unwrap_or_default())
                                    .flex_1()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM),
                            ),
                    )
                    .child(
                        text(move || tr("settings.loginMfaBackground"))
                            .text_xs()
                            .line_height(px(16.0))
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(
                        ui::button(move || tr("common.cancel"), ui::ButtonVariant::Secondary)
                            .disabled(state.busy)
                            .on_press(move || dispatch(Action::CancelMfa)),
                    )
            },
        ))
        .child(ui::status(state.feedback))
}

fn label(key: &'static str) -> Text {
    text(move || tr(key))
        .text_sm()
        .line_height(nsh_core::LineHeight::Multiple(1.375))
        .font_weight(FontWeight::MEDIUM)
}

fn masked(mobile: &str) -> String {
    if model::valid_mobile(mobile) {
        format!("{}****{}", &mobile[..3], &mobile[7..])
    } else {
        mobile.to_owned()
    }
}
