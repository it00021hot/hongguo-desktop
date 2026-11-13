use super::menus::{self, MenuAction, MenuState};
use crate::ui;
use hongguo_common::model::VideoDefinition;
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn Host() -> impl IntoElement {
    let rate = signal(1.0_f32);
    let definition = signal(None);
    let current = signal(1_u32);
    let busy = signal(false);
    let background = signal(0_u32);
    on_key("up", move |_| background.update(|count| *count += 1));
    on_key("down", move |_| background.update(|count| *count += 1));
    let state = MenuState {
        open: signal(None),
        definitions: computed(|| {
            vec![
                VideoDefinition {
                    value: 720,
                    width: 720,
                    height: 1280,
                },
                VideoDefinition {
                    value: 1080,
                    width: 1080,
                    height: 1920,
                },
            ]
        }),
        selected_definition: computed(move || definition.get()),
        seasons: super::SeasonState::empty(),
        episodes: computed(|| (1..=80).collect()),
        current_episode: computed(move || current.get()),
        volume: signal(1.0),
        muted: signal(false),
    };
    let dispatch = move |action| match action {
        MenuAction::SetRate(value) => rate.set(value),
        MenuAction::SetDefinition(value) => definition.set(value),
        MenuAction::SelectEpisode(value) => current.set(value),
        MenuAction::SetVolume(value) => {
            state.volume.set(value);
            state.muted.set(false);
        }
        MenuAction::ToggleMute => state.muted.update(|muted| *muted = !*muted),
        MenuAction::SwitchSeason(_) | MenuAction::ReserveSeason(_) | MenuAction::RetrySeasons => {}
    };
    view()
        .flex_1()
        .gap_2()
        .child(text(move || format!("rate:{}", rate.get())))
        .child(text(move || format!("definition:{:?}", definition.get())))
        .child(text(move || format!("episode:{}", current.get())))
        .child(text(move || format!("volume:{:.2}", state.volume.get())))
        .child(text(move || format!("muted:{}", state.muted.get())))
        .child(text(move || format!("background:{}", background.get())))
        .child(ui::button("Loading", ui::ButtonVariant::Quiet).on_press(move || busy.set(true)))
        .child(menus::rate(
            state,
            rate,
            computed(move || busy.get()),
            ui::player_control("Speed"),
            dispatch,
        ))
        .child(menus::quality(
            state,
            computed(move || busy.get()),
            ui::player_control("Quality"),
            dispatch,
        ))
        .child(menus::episodes(
            state,
            computed(move || busy.get()),
            ui::player_control("Episodes"),
            dispatch,
        ))
        .child(menus::volume(state, ui::player_control("Volume"), dispatch))
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<Host>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Speed")?;
    Ok(app)
}

#[test]
fn speed_and_quality_report_real_choices_and_expose_the_current_radio() -> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[Speed]")?;
    app.wait_for("role:menuitemradio[1.5x]")?;
    app.press("role:menuitemradio[1.5x]")?;
    app.wait_for("text:rate:1.5")?;
    app.press("role:button[Speed]")?;
    app.wait_for("role:menuitemradio[1.5x]")?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:menuitemradio[1.5x]")?
            .checked,
        Some(nsh_core::Checked::True)
    );
    app.press_key("Escape")?;
    app.press("role:button[Quality]")?;
    app.wait_for("role:menuitemradio[1080P]")?;
    app.press("role:menuitemradio[1080P]")?;
    app.wait_for("text:definition:Some(1080)")?;
    app.press("role:button[Quality]")?;
    app.press("role:menuitemradio[Auto]")?;
    app.wait_for("text:definition:None")?;
    app.press("role:button[Loading]")?;
    assert!(!app.screen().enabled("role:button[Quality]")?);
    assert!(!app.screen().enabled("role:button[Speed]")?);
    Ok(())
}

#[test]
fn episode_groups_keep_the_popup_open_and_follow_the_selected_episode() -> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[Episodes]")?;
    app.wait_for("role:radio[31-60]")?;
    app.press("role:radio[31-60]")?;
    app.wait_for("role:button[Ep. 31]")?;
    app.press("role:button[Ep. 31]")?;
    app.wait_for("text:episode:31")?;
    assert!(app.screen().selected("role:button[Ep. 31]")?);
    assert!(app.screen().matches("id:player-episode-panel")?);
    app.press_key("Escape")?;
    app.press("role:button[Episodes]")?;
    app.wait_for("role:button[Ep. 31]")?;
    app.press_key("right")?;
    app.wait_for("text:episode:32")?;
    app.press_key("down")?;
    app.wait_for("text:episode:40")?;
    assert!(app.screen().matches("text:background:0")?);
    app.press_key("Escape")?;
    app.wait_for_gone("id:player-episode-panel", Duration::from_secs(1))?;
    Ok(())
}

#[test]
fn volume_keyboard_and_mute_actions_flow_back_into_the_panel() -> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[Volume]")?;
    app.wait_for("id:player-volume-panel")?;
    app.press_key("down")?;
    assert!(app.screen().matches("text:background:0")?);
    app.press_key("tab")?;
    app.press_key("down")?;
    app.wait_for("text:volume:0.99")?;
    app.press("role:button[Mute]")?;
    app.wait_for("text:muted:true")?;
    app.press("role:button[Unmute]")?;
    app.wait_for("text:muted:false")?;
    Ok(())
}

#[test]
fn menu_arrow_keys_move_focus_and_enter_selects_without_advancing_the_background_feed()
-> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[Speed]")?;
    app.wait_for("role:menuitemradio[1.25x]")?;
    app.press_key("down")?;
    app.press_key("enter")?;
    app.wait_for("text:rate:1.25")?;
    assert!(app.screen().matches("text:background:0")?);
    app.press_key("enter")?;
    app.wait_for("role:menuitemradio[1.25x]")?;
    app.press_key("escape")?;
    app.press_key("down")?;
    app.wait_for("text:background:1")?;
    Ok(())
}
