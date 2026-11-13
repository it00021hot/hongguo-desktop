use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn Host() -> impl IntoElement {
    let feed_count = signal(0_i32);
    let episode = signal(1_i32);
    let taps = signal(0_u32);
    let blocked = signal(false);
    let active = signal(true);
    let feed = FeedNavigation::new(move |delta| {
        feed_count.update(|count| *count += delta);
        true
    });
    let controller = Controller::new(
        State {
            active: computed(move || active.get()),
            blocked: computed(move || blocked.get()),
            series: computed(|| "series-one".to_owned()),
        },
        Some(feed),
        move |delta| episode.update(|episode| *episode += delta),
    );
    view()
        .flex_1()
        .child(text(move || {
            format!(
                "feed:{} episode:{} taps:{}",
                feed_count.get(),
                episode.get(),
                taps.get()
            )
        }))
        .child(
            controller.bind(
                view()
                    .test_id("video-surface")
                    .h(px(400.0))
                    .w_full()
                    .pointer_events(PointerEvents::BoxOnly)
                    .on_press(move || taps.update(|taps| *taps += 1)),
            ),
        )
        .child(
            crate::ui::button("Lock", crate::ui::ButtonVariant::Quiet)
                .on_press(move || controller.enter_binge()),
        )
        .child(
            crate::ui::button("Unlock", crate::ui::ButtonVariant::Quiet)
                .on_press(move || controller.exit_binge()),
        )
        .child(
            crate::ui::button("Wheel down", crate::ui::ButtonVariant::Quiet)
                .on_press(move || controller.gesture(model::wheel(15.0))),
        )
        .child(
            crate::ui::button("Keyboard next", crate::ui::ButtonVariant::Quiet)
                .on_press(move || controller.step_keyboard(1)),
        )
        .child(
            crate::ui::button("Panel", crate::ui::ButtonVariant::Quiet)
                .on_press(move || blocked.update(|blocked| *blocked = !*blocked)),
        )
        .child(
            crate::ui::button("Leave", crate::ui::ButtonVariant::Quiet)
                .on_press(move || active.set(false)),
        )
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(Root::of::<Host>());
    config.viewport = nsh_engine::Size::new(px(480.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::Ios;
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:feed:0 episode:1 taps:0")?;
    Ok(app)
}

#[test]
fn a_vertical_swipe_steps_feed_then_locked_episodes_without_toggling_playback() -> Result<(), String>
{
    let mut app = start()?;
    app.drag("id:video-surface", point(px(0.0), px(-80.0)))?;
    app.wait_for("text:feed:1 episode:1 taps:0")?;
    app.press("Lock")?;
    app.run_for(Duration::from_millis(400))?;
    app.drag("id:video-surface", point(px(0.0), px(-80.0)))?;
    app.wait_for("text:feed:1 episode:2 taps:0")?;
    app.press("Unlock")?;
    app.run_for(Duration::from_millis(400))?;
    app.drag("id:video-surface", point(px(0.0), px(80.0)))?;
    app.wait_for("text:feed:0 episode:2 taps:0")?;
    Ok(())
}

#[test]
fn panel_and_inactive_routes_block_navigation_but_keyboard_does_not_take_the_gesture_cooldown()
-> Result<(), String> {
    let mut app = start()?;
    app.press("Wheel down")?;
    app.wait_for("text:feed:1 episode:1 taps:0")?;
    app.press("Keyboard next")?;
    app.wait_for("text:feed:2 episode:1 taps:0")?;
    app.press("Wheel down")?;
    assert!(app.screen().shows("feed:2 episode:1 taps:0"));
    app.press("Panel")?;
    app.run_for(Duration::from_millis(400))?;
    app.drag("id:video-surface", point(px(0.0), px(-80.0)))?;
    app.press("Keyboard next")?;
    assert!(app.screen().shows("feed:2 episode:1 taps:0"));
    app.press("Leave")?;
    app.press("Panel")?;
    app.press("Keyboard next")?;
    assert!(app.screen().shows("feed:2 episode:1 taps:0"));
    Ok(())
}

#[test]
fn horizontal_system_gestures_are_not_claimed_and_a_tap_keeps_its_existing_action()
-> Result<(), String> {
    let mut app = start()?;
    app.drag("id:video-surface", point(px(100.0), px(-60.0)))?;
    assert!(app.screen().shows("feed:0 episode:1 taps:0"));
    app.press("id:video-surface")?;
    app.wait_for("text:feed:0 episode:1 taps:1")?;
    Ok(())
}
