use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn LoginHost() -> impl IntoElement {
    let dialogs = preview_dialogs();
    let nav = Navigation::new();
    view()
        .flex_1()
        .child(
            ui::button("Open account", ui::ButtonVariant::Secondary)
                .on_press(move || nav.push(crate::navigation::Route::Account)),
        )
        .child(text(move || format!("routes:{:?}", nav.routes().get())))
        .child(card())
        .child(dialogs)
}

fn start<C: Component>() -> Result<Headless, String>
where
    C::Props: Default,
{
    let mut config = EngineConfig::new(Root::of::<C>().tokens(nsh_base::tokens_of(Some(
        include_str!("../../../../theme.json"),
    ))?));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.set_viewport(size(px(1000.0), px(840.0)))?;
    app.wait_for("Open account")?;
    Ok(app)
}

#[test]
fn every_account_request_opens_the_same_dialog_without_navigating_and_fields_validate()
-> Result<(), String> {
    let mut app = start::<LoginHost>()?;
    app.press("Open account")?;
    app.wait_for("id:login-dialog")?;
    assert!(
        app.screen()
            .accessibility_state("id:login-dialog >> role:button[Sign in]")?
            .disabled
    );
    assert!(
        app.screen()
            .accessibility_state("role:button[Send code]")?
            .disabled
    );
    app.type_text("Phone number", "13800138000")?;
    assert!(
        !app.screen()
            .accessibility_state("role:button[Send code]")?
            .disabled
    );
    app.type_text("SMS code", "1234")?;
    assert!(
        !app.screen()
            .accessibility_state("id:login-dialog >> role:button[Sign in]")?
            .disabled
    );
    app.press_key("Escape")?;
    app.wait_for_gone("id:login-dialog", std::time::Duration::from_secs(1))?;
    app.wait_for("text:routes:[Catalog]")?;
    app.press("role:button[Sign in]")?;
    app.wait_for("id:login-dialog")?;
    assert!(
        !app.screen()
            .accessibility_state("id:login-dialog >> role:button[Sign in]")?
            .disabled
    );
    Ok(())
}

#[test]
fn sending_code_preserves_phone_specific_cooldown_after_closing_and_changing_number()
-> Result<(), String> {
    let mut app = start::<LoginHost>()?;
    app.press("Open account")?;
    app.type_text("Phone number", "13800138000")?;
    app.press("role:button[Send code]")?;
    app.wait_for("Resend in 60s")?;
    app.press_key("Escape")?;
    app.press("Open account")?;
    app.wait_for("id:login-dialog")?;
    assert!(!app.shows("role:button[Send code]"));
    app.clear("Phone number")?;
    app.type_text("Phone number", "13900139000")?;
    app.wait_for("role:button[Send code]")?;
    assert!(
        !app.screen()
            .accessibility_state("role:button[Send code]")?
            .disabled
    );
    app.clear("Phone number")?;
    app.type_text("Phone number", "13800138000")?;
    assert!(!app.shows("role:button[Send code]"));
    Ok(())
}

#[component]
fn MfaHost() -> impl IntoElement {
    let prompt = AccountPrompt::provide();
    let mfa = signal(Some(SharedString::from("Reply YZ to 106900000")));
    let state = State::new(computed(move || mfa.get()));
    state.mobile.set("13800138000".into());
    view()
        .flex_1()
        .child(
            ui::button("Open account", ui::ButtonVariant::Secondary)
                .on_press(move || prompt.request()),
        )
        .child(ui::login_dialog(
            computed(move || prompt.open()),
            move || prompt.set_open(false),
            move || {
                view::form(state, move |action| {
                    if matches!(action, Action::CancelMfa) {
                        mfa.set(None);
                    } else {
                        panic!("MFA waits without resending credentials");
                    }
                })
            },
        ))
}

#[test]
fn closing_the_login_dialog_keeps_mfa_waiting_and_only_explicit_cancel_restores_the_form()
-> Result<(), String> {
    let mut app = start::<MfaHost>()?;
    app.press("Open account")?;
    app.wait_for("Reply YZ to 106900000")?;
    app.press("role:button[Close]")?;
    app.wait_for_gone("id:login-dialog", std::time::Duration::from_secs(1))?;
    app.press("Open account")?;
    app.wait_for("Reply YZ to 106900000")?;
    app.press("role:button[Cancel]")?;
    app.wait_for("SMS code")?;
    assert!(!app.shows("Reply YZ to 106900000"));
    Ok(())
}
