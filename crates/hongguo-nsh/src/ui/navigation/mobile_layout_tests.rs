use super::*;
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn MobileLayout() -> impl IntoElement {
    let nav = Navigation::new();
    let selected = signal(false);
    let height = signal(0.0_f32);
    let target = signal(0.0_f32);
    let content = view()
        .flex_1()
        .min_h_0()
        .on_layout(move |bounds: &Bounds<Points>| height.set(bounds.size.height.0))
        .child(dynamic(move || {
            mobile_catalog_header(
                view().flex_row().child(
                    view()
                        .on_layout(move |bounds: &Bounds<Points>| target.set(bounds.size.height.0))
                        .child(
                            mobile_channel_tab("Channel", selected)
                                .on_press(move || selected.set(true)),
                        ),
                ),
            )
        }))
        .child(text(move || {
            format!(
                "height:{} target:{} selected:{}",
                height.get(),
                target.get(),
                selected.get()
            )
        }))
        .child(
            button("Open player", ButtonVariant::Quiet).on_press(move || {
                nav.push(Route::Player("fixture".into(), 1));
            }),
        )
        .child(button("Back", ButtonVariant::Quiet).on_press(move || nav.back()));
    shell(nav, view(), content)
}

#[test]
fn mobile_catalog_targets_fit_one_row_and_bottom_inset_belongs_to_the_visible_bar()
-> Result<(), String> {
    for (os, target) in [(Os::Ios, 44), (Os::Android, 48)] {
        let root = Root::of::<MobileLayout>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap());
        let mut config = EngineConfig::new(root);
        config.environment.os = os;
        config.environment.reduce_motion = true;
        config.environment.safe_area.bottom = px(34.0);
        config.viewport = nsh_engine::Size::new(px(320.0), px(840.0));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for(&format!("text:height:840 target:{target} selected:false"))?;
        app.press("role:button[Channel]")?;
        app.wait_for(&format!("text:height:840 target:{target} selected:true"))?;
        app.press("role:button[Main menu]")?;
        app.wait_for("id:mobile-main-menu")?;
        app.back()?;
        app.wait_for_gone("id:mobile-main-menu", std::time::Duration::from_secs(1))?;
        app.press("Open player")?;
        app.wait_for_gone("id:main-nav", std::time::Duration::from_secs(1))?;
        app.wait_for(&format!("text:height:806 target:{target} selected:true"))?;
        app.press("Back")?;
        app.wait_for(&format!("text:height:840 target:{target} selected:true"))?;
    }
    Ok(())
}
