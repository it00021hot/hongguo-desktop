use super::{ControlAction, ControlState, MenuState, controls, surface};
use crate::ui;
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn LaggingNativePosition() -> impl IntoElement {
    let position = signal((20.0_f64, 169.0_f64));
    let pending = signal(false);
    let requested = signal(None::<f64>);
    let state = ControlState {
        geometry: ui::PlayerGeometry {
            source: signal((720.0, 1280.0)),
            viewport: signal((0.0, 0.0)),
        },
        caption: computed(ui::WindowCaption::default),
        paused: signal(true),
        rate: signal(1.0),
        position,
        quality: computed(|| "720P".into()),
        busy: computed(|| false),
        seek_pending: computed(move || pending.get()),
        has_previous: computed(|| false),
        has_next: computed(|| false),
        episode_count: computed(|| 1),
        can_binge: false,
        in_binge: computed(|| false),
        advanced: super::AdvancedState::unavailable(),
        danmaku_settings: crate::ui::DanmakuSettingsState::new(),
        danmaku_enabled: signal(true),
        menus: MenuState {
            open: signal(None),
            definitions: computed(Vec::new),
            selected_definition: computed(|| None),
            seasons: super::SeasonState::empty(),
            episodes: computed(|| vec![1]),
            current_episode: computed(|| 1),
            volume: signal(1.0),
            muted: signal(false),
        },
    };
    view()
        .flex_1()
        .child(text(move || format!("requested:{:?}", requested.get())))
        .child(surface(
            view(),
            view(),
            view(),
            controls(
                state,
                move |action| {
                    if let ControlAction::Seek(at) = action {
                        requested.set(Some(at));
                        pending.set(true);
                    }
                },
                |_| view(),
            ),
            view(),
        ))
        .child(
            ui::button("Confirm native seek", ui::ButtonVariant::Quiet).on_press(move || {
                if let Some(at) = requested.get_untracked() {
                    position.set((at, 169.0));
                    pending.set(false);
                }
            }),
        )
}

#[test]
fn the_toolbar_and_binge_entry_share_one_episode_popup() -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<LaggingNativePosition>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:00:20 / 02:49")?;
    app.press("role:button[Episodes · 1 episodes]")?;
    app.wait_for("id:player-episode-panel")?;
    assert_eq!(app.screen().count("id:player-episode-panel")?, 1);
    Ok(())
}

#[test]
fn rapid_seek_keys_keep_the_draft_until_native_position_confirmation() -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<LaggingNativePosition>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:00:20 / 02:49")?;
    app.press_key("tab")?;
    app.press_key("home")?;
    app.wait_for("text:requested:Some(0.0)")?;
    app.press_key("right")?;
    app.wait_for("text:requested:Some(1.0)")?;
    // Transport requests do not invent native progress while the seek is in flight.
    assert!(app.screen().matches("text:00:20 / 02:49")?);
    app.press("role:button[Confirm native seek]")?;
    app.wait_for("text:00:01 / 02:49")?;
    app.press_key("tab")?;
    app.press_key("right")?;
    app.wait_for("text:requested:Some(2.0)")?;
    Ok(())
}

#[test]
fn pointer_seek_keeps_the_draft_until_native_position_confirmation() -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<LaggingNativePosition>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("text:00:20 / 02:49")?;
    assert!(app.screen().matches("id:player-seek-rail")?);
    app.drag(
        "role:adjustable[Playback position]",
        nsh_core::Point::new(px(64.0), px(0.0)),
    )?;
    app.wait_for("text:requested:Some(29.0)")?;
    app.press("role:button[Confirm native seek]")?;
    app.wait_for("text:00:29 / 02:49")?;
    app.drag(
        "role:adjustable[Playback position]",
        nsh_core::Point::new(px(-64.0), px(0.0)),
    )?;
    app.wait_for("text:requested:Some(20.0)")?;
    assert!(app.screen().matches("text:00:29 / 02:49")?);
    app.press("role:button[Confirm native seek]")?;
    app.wait_for("text:00:20 / 02:49")?;
    Ok(())
}
