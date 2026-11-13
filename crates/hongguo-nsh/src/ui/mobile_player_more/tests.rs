use super::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

const RATES: &[f32] = &[0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

#[component]
fn Host() -> impl IntoElement {
    let open = signal(false);
    let rate = signal(1.0_f32);
    let definition = signal(None);
    let danmaku_on = signal(true);
    let danmaku = DanmakuSettingsState::new();
    let event = signal(String::new());
    let background = signal(0_u32);
    on_key("down", move |_| background.update(|value| *value += 1));
    let state = MobilePlayerMoreState {
        open: computed(move || open.get()),
        rate,
        rates: RATES,
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
        danmaku_on,
        danmaku,
        busy: computed(|| false),
        episode_count: computed(|| 80),
        enhancement_available: computed(|| false),
    };
    view()
        .flex_1()
        .child(mobile_player_more(
            state,
            MobilePlayerMoreEvents {
                change: Box::new(move |next| open.set(next)),
                action: Box::new(move |action| match action {
                    MobilePlayerMoreAction::SetRate(value) => rate.set(value),
                    MobilePlayerMoreAction::SetDefinition(value) => definition.set(value),
                    MobilePlayerMoreAction::ToggleDanmaku => {
                        danmaku_on.update(|enabled| *enabled = !*enabled)
                    }
                    other => event.set(format!("{other:?}")),
                }),
            },
            super::super::player_phone_rate("Speed"),
            element_ref(),
        ))
        .child(text(move || format!("rate:{}", rate.get())))
        .child(text(move || format!("definition:{:?}", definition.get())))
        .child(text(move || format!("danmaku:{}", danmaku_on.get())))
        .child(text(move || {
            format!("opacity:{:.2}", danmaku.opacity.get())
        }))
        .child(text(move || format!("background:{}", background.get())))
        .child(text(move || event.get()))
}

fn start() -> Result<Headless, String> {
    let root = Root::of::<Host>().tokens(nsh_base::tokens_of(Some(include_str!(
        "../../../theme.json"
    )))?);
    let mut config = EngineConfig::new(root);
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.set_viewport(size(px(480.0), px(840.0)))?;
    app.wait_for("Speed")?;
    Ok(app)
}

#[test]
fn mobile_more_preserves_shared_values_and_closes_before_opening_another_panel()
-> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[Speed]")?;
    app.wait_for("id:mobile-player-more")?;
    app.press("role:radio[1.5x]")?;
    app.wait_for("text:rate:1.5")?;
    app.press_key("right")?;
    app.wait_for("text:rate:2")?;
    app.press("role:radio[1080P]")?;
    app.wait_for("text:definition:Some(1080)")?;
    app.press("role:radio[Auto]")?;
    app.wait_for("text:definition:None")?;
    app.press("role:switch[Danmaku]")?;
    app.wait_for("text:danmaku:false")?;
    assert!(!app.screen().enabled("role:button[Video enhancement]")?);
    app.press("role:button[Danmaku settings]")?;
    app.wait_for("role:adjustable[Opacity]")?;
    app.drag("role:adjustable[Opacity]", point(px(100.0), px(0.0)))?;
    assert!(!app.shows("text:opacity:0.50"));
    app.press_key("escape")?;
    app.wait_for_gone("id:mobile-player-more", Duration::from_secs(1))?;
    app.press("role:button[Speed]")?;
    app.wait_for("role:radio[2x]")?;
    assert!(!app.shows("role:adjustable[Opacity]"));
    app.press("role:button[Download to local]")?;
    app.wait_for("text:Download")?;
    app.wait_for_gone("id:mobile-player-more", Duration::from_secs(1))?;
    app.wait_for("text:background:0")?;
    Ok(())
}
