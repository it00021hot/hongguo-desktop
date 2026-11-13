use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn FastVideo() -> impl IntoElement {
    let position = signal((0.0, 100.0));
    let paused = signal(false);
    let waiting = signal(false);
    let seeking = signal(None);
    let timeline = signal(Timeline::new("series".into(), "episode".into()));
    timeline.update(|timeline| {
        timeline.insert(Danmaku {
            comment_id: "wall-clock".into(),
            text: "wall clock caption".into(),
            offset_ms: 0,
            digg_count: 0,
        })
    });
    let preferences = DanmakuPreferences::new();
    preferences.display.density.set(1.0);
    let handle = Handle {
        enabled: preferences.enabled,
        timeline,
        position,
        motion: Motion::new(
            computed(|| true),
            paused,
            computed(|| false),
            waiting,
            seeking,
        ),
        preferences,
        error: signal(SharedString::default()),
        retry: signal(0),
    };
    set_interval(Duration::from_millis(250), move || {
        if !paused.get_untracked() && !waiting.get_untracked() && seeking.get_untracked().is_none()
        {
            position.update(|position| position.0 += 0.5);
        }
    });
    view()
        .w(px(500.0))
        .h(px(300.0))
        .child(handle.overlay())
        .child(
            crate::ui::button("Begin waiting", crate::ui::ButtonVariant::Quiet)
                .on_press(move || waiting.set(true)),
        )
        .child(
            crate::ui::button("End waiting", crate::ui::ButtonVariant::Quiet)
                .on_press(move || waiting.set(false)),
        )
        .child(
            crate::ui::button("Begin small seek", crate::ui::ButtonVariant::Quiet)
                .on_press(move || seeking.set(Some(position.get_untracked().0 + 0.2))),
        )
        .child(
            crate::ui::button("Confirm small seek", crate::ui::ButtonVariant::Quiet).on_press(
                move || {
                    batch(|| {
                        if let Some(at) = seeking.get_untracked() {
                            position.update(|position| position.0 = at);
                        }
                        seeking.set(None);
                    })
                },
            ),
        )
        .child(
            crate::ui::button("Pause captions", crate::ui::ButtonVariant::Quiet)
                .on_press(move || paused.set(true)),
        )
        .child(
            crate::ui::button("Resume captions", crate::ui::ButtonVariant::Quiet)
                .on_press(move || paused.set(false)),
        )
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<FastVideo>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(600.0), px(500.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("wall clock caption")?;
    Ok(app)
}

#[test]
fn two_times_playback_keeps_the_caption_until_nine_wall_seconds() -> Result<(), String> {
    let mut app = start()?;
    app.wait(Duration::from_millis(5000))?;
    assert!(
        app.screen().matches("wall clock caption")?,
        "10 media seconds must not expire a flight after only 5 wall seconds"
    );
    app.wait_for_gone("wall clock caption", Duration::from_millis(5000))?;
    app.press("Pause captions")?;
    Ok(())
}

#[test]
fn a_paused_flight_does_not_expire_and_resumes_only_its_remaining_duration() -> Result<(), String> {
    let mut app = start()?;
    app.wait(Duration::from_millis(3000))?;
    app.press("Pause captions")?;
    app.wait(Duration::from_millis(11000))?;
    assert!(app.screen().matches("wall clock caption")?);
    app.press("Resume captions")?;
    app.wait(Duration::from_millis(4000))?;
    assert!(app.screen().matches("wall clock caption")?);
    app.wait_for_gone("wall clock caption", Duration::from_millis(3000))?;
    Ok(())
}

#[test]
fn buffering_freezes_existing_cues_without_resetting_their_remaining_time() -> Result<(), String> {
    let mut app = start()?;
    app.wait(Duration::from_secs(3))?;
    app.press("Begin waiting")?;
    app.wait(Duration::from_secs(11))?;
    assert!(app.screen().matches("wall clock caption")?);
    app.press("End waiting")?;
    app.wait(Duration::from_secs(4))?;
    assert!(app.screen().matches("wall clock caption")?);
    app.wait_for_gone("wall clock caption", Duration::from_secs(3))?;
    Ok(())
}

#[test]
fn an_explicit_subsecond_seek_clears_old_cues_before_any_native_progress_and_does_not_replay_them()
-> Result<(), String> {
    let mut app = start()?;
    app.press("Begin small seek")?;
    app.wait(Duration::from_millis(20))?;
    assert!(
        !app.screen().matches("wall clock caption")?,
        "a subsecond seek must discard the old flight before a tick or its natural expiry"
    );
    app.press("Confirm small seek")?;
    app.wait(Duration::from_millis(500))?;
    assert!(!app.screen().matches("wall clock caption")?);
    Ok(())
}
