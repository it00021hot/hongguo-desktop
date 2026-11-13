use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Overlay() -> impl IntoElement {
    let active = signal(true);
    let paused = signal(false);
    let panel = signal(false);
    let chrome = Chrome::new(State {
        active: computed(move || active.get()),
        paused,
        panel_open: computed(move || panel.get()),
    });
    let action = |label, callback: Box<dyn Fn()>| {
        crate::ui::button(label, crate::ui::ButtonVariant::Quiet).on_press(callback)
    };
    view()
        .child(text(move || {
            if chrome.shown().get() {
                "shown"
            } else {
                "hidden"
            }
        }))
        .child(action("Wake", Box::new(move || chrome.wake())))
        .child(action("Move", Box::new(move || chrome.moved())))
        .child(action("Leave", Box::new(move || chrome.leave())))
        .child(action("Pause", Box::new(move || paused.set(true))))
        .child(action("Play", Box::new(move || paused.set(false))))
        .child(action("Hover", Box::new(move || chrome.hovered(true))))
        .child(action("Unhover", Box::new(move || chrome.hovered(false))))
        .child(action("Open", Box::new(move || panel.set(true))))
        .child(action("Close", Box::new(move || panel.set(false))))
        .child(action("Quiet", Box::new(move || chrome.arm_quiet())))
        .child(action("Played", Box::new(move || chrome.mark_played())))
        .child(action("Deactivate", Box::new(move || active.set(false))))
        .child(action("Activate", Box::new(move || active.set(true))))
        .child(chrome.bind_focus(view().child(crate::ui::field("Control input"))))
}

fn app() -> Result<Headless, String> {
    let mut config = EngineConfig::new(Root::of::<Overlay>());
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:shown")?;
    Ok(app)
}

#[test]
fn playing_idle_hides_after_three_seconds_and_movement_restarts_the_deadline() -> Result<(), String>
{
    let mut app = app()?;
    app.wait_for("text:shown")?;
    app.run_for(HIDE_AFTER + Duration::from_millis(100))?;
    app.wait_for("text:hidden")?;
    app.press("Wake")?;
    app.run_for(Duration::from_secs(2))?;
    app.press("Move")?;
    app.run_for(Duration::from_secs(2))?;
    assert!(app.screen().matches("text:shown")?);
    app.run_for(Duration::from_millis(1200))?;
    app.wait_for("text:hidden")?;
    Ok(())
}

#[test]
fn pause_hover_and_open_panels_keep_the_chrome_visible() -> Result<(), String> {
    for (open, close) in [("Pause", "Play"), ("Hover", "Unhover"), ("Open", "Close")] {
        let mut app = app()?;
        app.press(open)?;
        app.run_for(HIDE_AFTER + Duration::from_millis(100))?;
        assert!(app.screen().matches("text:shown")?);
        app.press(close)?;
        app.run_for(HIDE_AFTER + Duration::from_millis(100))?;
        app.wait_for("text:hidden")?;
    }
    Ok(())
}

#[test]
fn leaving_hides_immediately_but_does_not_override_pause_or_a_panel() -> Result<(), String> {
    let mut app = app()?;
    app.press("Leave")?;
    app.wait_for("text:hidden")?;
    app.press("Pause")?;
    app.wait_for("text:shown")?;
    app.press("Leave")?;
    assert!(app.screen().matches("text:shown")?);
    app.press("Play")?;
    app.press("Open")?;
    app.wait_for("text:shown")?;
    app.press("Deactivate")?;
    app.wait_for("text:hidden")?;
    app.press("Activate")?;
    app.wait_for("text:shown")?;
    Ok(())
}

#[test]
fn automatic_advance_ignores_phantom_moves_until_played_or_explicit_input() -> Result<(), String> {
    let mut app = app()?;
    app.press("Quiet")?;
    app.press("Pause")?;
    app.press("Move")?;
    app.wait_for("text:hidden")?;
    app.press("Open")?;
    app.wait_for("text:shown")?;
    app.press("Close")?;
    app.wait_for("text:hidden")?;
    app.press("Wake")?;
    app.wait_for("text:shown")?;
    app.press("Quiet")?;
    app.press("Played")?;
    app.wait_for("text:shown")?;
    Ok(())
}

#[test]
fn focused_controls_do_not_disappear_during_editing() -> Result<(), String> {
    let mut app = app()?;
    app.type_text("Control input", "text")?;
    app.run_for(HIDE_AFTER + Duration::from_millis(100))?;
    app.wait_for("text:shown")?;
    app.press_key("tab")?;
    app.press("Wake")?;
    app.run_for(HIDE_AFTER + Duration::from_millis(100))?;
    app.wait_for("text:hidden")?;
    Ok(())
}

#[component]
fn RebuiltChrome() -> impl IntoElement {
    let owner = signal(0_u64);
    let paused = signal(false);
    let feed = crate::player::navigation::FeedNavigation::new(|_| true);
    feed.on_goto(move |_, _| {
        feed.invalidate();
        owner.update(|owner| *owner += 1);
    });
    let handoff = feed.quiet_handoff();
    view().child(each(
        move || vec![owner.get()],
        |owner: &u64| *owner,
        move |generation, _| {
            let chrome = Chrome::new(State {
                active: computed(|| true),
                paused,
                panel_open: computed(|| false),
            })
            .with_handoff(Some(handoff));
            view()
                .child(text(move || format!("owner:{}", generation.get())))
                .child(text(move || {
                    if chrome.shown().get() {
                        "shown"
                    } else {
                        "hidden"
                    }
                }))
                .child(
                    crate::ui::button("Quiet", crate::ui::ButtonVariant::Quiet)
                        .on_press(move || chrome.arm_quiet()),
                )
                .child(
                    crate::ui::button("Wake", crate::ui::ButtonVariant::Quiet)
                        .on_press(move || chrome.wake()),
                )
                .child(
                    crate::ui::button("Played", crate::ui::ButtonVariant::Quiet)
                        .on_press(move || chrome.mark_played()),
                )
                .child(
                    crate::ui::button("Remount", crate::ui::ButtonVariant::Quiet).on_press(
                        move || {
                            feed.request_goto(Default::default(), 1);
                        },
                    ),
                )
                .child(
                    crate::ui::button("Manual context", crate::ui::ButtonVariant::Quiet).on_press(
                        move || {
                            feed.invalidate();
                            owner.update(|owner| *owner += 1);
                        },
                    ),
                )
                .child(
                    crate::ui::button("Auto feed", crate::ui::ButtonVariant::Quiet).on_press(
                        move || {
                            if feed.advance_feed() {
                                feed.invalidate();
                                owner.update(|owner| *owner += 1);
                            }
                        },
                    ),
                )
        },
    ))
}

fn rebuilt() -> Result<Headless, String> {
    let mut config = EngineConfig::new(Root::of::<RebuiltChrome>());
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:owner:0")?;
    Ok(app)
}

#[test]
fn an_automatic_feed_owner_rebuild_inherits_quiet_once_without_flashing_controls()
-> Result<(), String> {
    let mut app = rebuilt()?;
    app.press("Quiet")?;
    assert!(app.screen().matches("text:hidden")?);
    app.press("Remount")?;
    app.wait_for("text:owner:1")?;
    assert!(
        app.screen().matches("text:hidden")?,
        "the new owner must inherit quiet before its first visible frame"
    );
    app.press("Remount")?;
    app.wait_for("text:owner:2")?;
    assert!(
        app.screen().matches("text:shown")?,
        "an already-consumed cue must not hide a later manual target"
    );
    Ok(())
}

#[test]
fn explicit_wake_or_real_playback_clears_the_pending_cross_owner_cue() -> Result<(), String> {
    for action in ["Wake", "Played"] {
        let mut app = rebuilt()?;
        app.press("Quiet")?;
        app.press(action)?;
        app.press("Remount")?;
        app.wait_for("text:owner:1")?;
        assert!(
            app.screen().matches("text:shown")?,
            "{action} must clear the shared auto-advance cue"
        );
    }
    Ok(())
}

#[test]
fn an_expired_quiet_deadline_is_not_restarted_for_the_new_owner() -> Result<(), String> {
    let mut app = rebuilt()?;
    app.press("Quiet")?;
    app.run_for(QUIET_WINDOW + Duration::from_millis(100))?;
    app.press("Remount")?;
    app.wait_for("text:owner:1")?;
    assert!(app.screen().matches("text:shown")?);
    Ok(())
}

#[test]
fn manual_feed_context_clears_an_unconsumed_cue_but_auto_feed_keeps_it() -> Result<(), String> {
    let mut app = rebuilt()?;
    app.press("Quiet")?;
    app.press("Manual context")?;
    app.wait_for("text:owner:1")?;
    assert!(
        app.screen().matches("text:shown")?,
        "a manual channel selection must not inherit an automatic cue"
    );
    let mut app = rebuilt()?;
    app.press("Quiet")?;
    app.press("Auto feed")?;
    app.wait_for("text:owner:1")?;
    assert!(app.screen().matches("text:hidden")?);
    Ok(())
}
