use super::{WindowModes, provide_preview};
use crate::{navigation::Navigation, ui};
use nsh_core::{element::ImageSrc, prelude::*};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[component]
fn FullscreenLayout() -> impl IntoElement {
    let driver = provide_preview();
    let modes = WindowModes::current().expect("preview window state");
    on_key("mod+alt+a", move |_| driver.acknowledge());
    let stage = crate::player::stage::fixture_with_window_modes(
        ImageSrc::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/nsh-layout-fixtures/player-frame.png"
        )),
        || {},
    );
    ui::shell(Navigation::new(), view(), stage).child(
        view()
            .absolute()
            .top_0()
            .left_0()
            .pointer_events(PointerEvents::None)
            .child(text(move || {
                format!(
                    "full:{} frame:{} pending:{}",
                    modes.state().fullscreen,
                    modes.state().frameless,
                    modes.pending()
                )
            })),
    )
}

#[derive(Default)]
struct Frames {
    parents: HashMap<nsh_core::NodeId, nsh_core::NodeId>,
    bounds: HashMap<nsh_core::NodeId, Bounds<Points>>,
}

impl Frames {
    fn root_bounds(&self, mut node: nsh_core::NodeId) -> Option<Bounds<Points>> {
        let mut bounds = *self.bounds.get(&node)?;
        while let Some(parent) = self.parents.get(&node) {
            if *parent == node {
                break;
            }
            if let Some(frame) = self.bounds.get(parent) {
                bounds.origin.x += frame.origin.x;
                bounds.origin.y += frame.origin.y;
            }
            node = *parent;
        }
        Some(bounds)
    }
}

fn right(app: &Headless, frames: &Arc<Mutex<Frames>>) -> Result<f32, String> {
    let node = app
        .screen()
        .press_target("role:button[全屏]")?
        .ok_or("fullscreen button missing")?;
    let bounds = frames
        .lock()
        .unwrap()
        .root_bounds(node)
        .ok_or("fullscreen button has no frame")?;
    Ok(bounds.origin.x.0 + bounds.size.width.0)
}

#[test]
fn desktop_fullscreen_keeps_the_trailing_controls_at_the_stage_edge() -> Result<(), String> {
    let (mut app, frames) = start(1280.0, 840.0)?;
    app.wait_for("id:player-stage")?;
    app.wait_for("role:button[全屏]")?;
    let normal = right(&app, &frames)?;
    assert!(
        (normal - 1264.0).abs() < 1.0,
        "normal trailing edge {normal}"
    );
    app.press("role:button[全屏]")?;
    app.press_key("mod+alt+a")?;
    app.wait_for("text:full:true frame:false pending:false")?;
    app.wait_for_gone("id:main-nav", Duration::from_secs(2))?;
    let fullscreen = right(&app, &frames)?;
    assert!(
        (fullscreen - 1264.0).abs() < 1.0,
        "fullscreen trailing edge {fullscreen}; expected1264, not the row's natural width"
    );
    app.set_viewport(nsh_engine::Size::new(px(1600.0), px(900.0)))?;
    let resized = right(&app, &frames)?;
    assert!(
        (resized - 1584.0).abs() < 1.0,
        "resized trailing edge {resized}"
    );
    app.press("role:button[全屏]")?;
    app.press_key("mod+alt+a")?;
    app.wait_for("text:full:false frame:false pending:false")?;
    app.wait_for("id:main-nav")?;
    assert_eq!(app.screen().count("id:player-stage")?, 1);
    assert!((right(&app, &frames)? - 1584.0).abs() < 1.0);
    Ok(())
}

#[test]
fn narrow_desktop_keeps_input_on_the_first_row_and_all_controls_inside_the_stage()
-> Result<(), String> {
    let (mut app, frames) = start(1100.0, 840.0)?;
    app.wait_for("role:button[全屏]")?;
    let bounds = |selector: &str| -> Result<Bounds<Points>, String> {
        let node = app
            .screen()
            .press_target(selector)?
            .ok_or("missing control")?;
        frames
            .lock()
            .unwrap()
            .root_bounds(node)
            .ok_or("missing frame".into())
    };
    let play = bounds("role:button[播放]")?;
    let fullscreen = bounds("role:button[全屏]")?;
    assert_eq!(fullscreen.origin.y - play.origin.y, px(36.0));
    assert!((right(&app, &frames)? - 1084.0).abs() < 1.0);
    app.type_text("label:发条友善的弹幕吧", "中文 Abc123")?;
    app.wait_for("text:中文 Abc123")?;
    assert!(!app.screen().shows("text:0.75x"));
    Ok(())
}

type ObservedFrames = Arc<Mutex<Frames>>;

fn start(width: f32, height: f32) -> Result<(Headless, ObservedFrames), String> {
    let mut config = EngineConfig::new(
        Root::of::<FullscreenLayout>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(width), px(height));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    let frames = Arc::new(Mutex::new(Frames::default()));
    let observed = frames.clone();
    app.observe(move |_, batch| {
        let mut frames = observed.lock().unwrap();
        for item in &batch.items {
            match item {
                nsh_core::MountItem::Insert { parent, child, .. } => {
                    frames.parents.insert(*child, *parent);
                }
                nsh_core::MountItem::Frame { id, frame } => {
                    frames.bounds.insert(*id, *frame);
                }
                _ => {}
            }
        }
    });
    Ok((app, frames))
}
