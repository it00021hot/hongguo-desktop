use super::{ControlAction, ControlState, MenuState, controls, surface};
use crate::ui;
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn LaggingNativePosition() -> impl IntoElement {
    let position = signal((20.0_f64, 169.0_f64));
    let pending = signal(false);
    let paused = signal(true);
    let requested = signal(None::<f64>);
    let state = ControlState {
        geometry: ui::PlayerGeometry {
            source: signal((720.0, 1280.0)),
            viewport: signal((0.0, 0.0)),
        },
        caption: computed(ui::WindowCaption::default),
        paused,
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
            view()
                .w_full()
                .h_full()
                .pointer_events(PointerEvents::BoxOnly)
                .focusable(false)
                .role(nsh_core::AccessibilityRole::Button)
                .label(move || {
                    crate::i18n::tr(if paused.get() {
                        "player.playVideo"
                    } else {
                        "player.pauseVideo"
                    })
                })
                .on_press(move || paused.update(|paused| *paused = !*paused)),
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
fn touch_seeking_claims_native_presses_without_changing_the_playback_intent() -> Result<(), String>
{
    let mut config = EngineConfig::new(
        Root::of::<LaggingNativePosition>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(393.0), px(852.0));
    config.environment.os = nsh_core::platform::Os::Android;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("role:adjustable[Playback position]")?;
    let native_boundary = app.screen().press_target("id:player-seek-rail")?;
    let thumb = app
        .screen()
        .press_target("role:adjustable[Playback position]")?;
    assert!(
        native_boundary.is_some(),
        "the rail must consume the native tap after the responder handles it"
    );
    assert_ne!(
        native_boundary, thumb,
        "the full rail, including space away from the thumb, owns the native tap"
    );
    app.press("id:player-seek-rail")?;
    assert!(app.screen().matches("role:button[Play video]")?);
    app.drag(
        "role:adjustable[Playback position]",
        nsh_core::Point::new(px(64.0), px(0.0)),
    )?;
    let forward = requested_position(&app)?;
    assert!(forward > 20.0 && forward < 169.0);
    assert!(app.screen().matches("role:button[Play video]")?);
    app.press("role:button[Play video]")?;
    app.wait_for("role:button[Pause video]")?;
    app.press("id:player-seek-rail")?;
    assert!(app.screen().matches("role:button[Pause video]")?);
    app.drag(
        "role:adjustable[Playback position]",
        nsh_core::Point::new(px(-64.0), px(0.0)),
    )?;
    assert!(requested_position(&app)? < forward);
    assert!(app.screen().matches("role:button[Pause video]")?);
    Ok(())
}

fn requested_position(app: &Headless) -> Result<f64, String> {
    app.screen()
        .texts()
        .iter()
        .find_map(|text| {
            text.strip_prefix("requested:Some(")
                .and_then(|value| value.strip_suffix(')'))
        })
        .ok_or_else(|| "the slider did not request a seek".to_owned())?
        .parse()
        .map_err(|error| format!("invalid requested seek: {error}"))
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
