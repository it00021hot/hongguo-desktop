use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Host() -> impl IntoElement {
    super::reviews_preview_with_header(view().child(text("详情页头")))
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<Host>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1000.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("我的评分")?;
    Ok(app)
}

#[test]
fn review_form_requires_stars_and_preserves_the_complete_echo_in_the_same_feed()
-> Result<(), String> {
    let mut app = start()?;
    app.type_text("写下你的剧评…", "完整剧评[微笑]")?;
    assert!(
        app.screen()
            .accessibility_state("role:button[发表剧评]")?
            .disabled
    );
    app.press_key("shift+tab")?;
    app.press_key("right")?;
    app.press_key("right")?;
    assert!(
        !app.screen()
            .accessibility_state("role:button[发表剧评]")?
            .disabled
    );
    app.press("role:button[发表剧评]")?;
    app.wait_for("id:comment-sent-100")?;
    app.wait_for("刚刚发表")?;
    app.wait_for("id:comment-sent-100 >> role:image[评 3 星]")?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:radio[评 3 星]")?
            .checked,
        Some(nsh_core::Checked::True)
    );
    app.type_text("写下你的剧评…", "保持当前评分")?;
    app.press_key("shift+tab")?;
    app.press_key("right")?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:radio[评 4 星]")?
            .checked,
        Some(nsh_core::Checked::True)
    );
    app.type_text("写下你的剧评…", "表情")?;
    app.press_key("tab")?;
    app.press_key("space")?;
    app.wait_for("role:button[[偷笑]]")?;
    app.press_key("escape")?;
    Ok(())
}

#[test]
fn review_rows_recycle_to_current_identity_and_keep_nonempty_reply_content() -> Result<(), String> {
    let mut app = start()?;
    app.wait_for("第1位观众")?;
    app.wait_for("回复的观众")?;
    app.press("id:comment-review-preview-0 >> role:button[点赞 12]")?;
    app.wait_for("id:comment-review-preview-0 >> role:button[已点赞 13]")?;
    app.scroll("id:reviews-list", px(7000.0))?;
    let current = app
        .screen()
        .texts()
        .into_iter()
        .find(|text| text.starts_with("第") && text.ends_with("位观众"))
        .ok_or("no recycled review row")?;
    assert_ne!(current, "第1位观众");
    app.scroll("id:reviews-list", px(-7000.0))?;
    app.wait_for("id:comment-review-preview-0 >> role:button[已点赞 13]")?;
    Ok(())
}

#[test]
fn clicking_a_rating_mounts_the_new_focus_stop_before_asking_native_to_focus_it()
-> Result<(), String> {
    use std::sync::{Arc, Mutex};
    let mut app = start()?;
    app.type_text("写下你的剧评…", "键盘评分")?;
    let radio = app
        .screen()
        .press_target("role:radio[评 2 星]")?
        .ok_or("rating missing")?;
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let observed = order.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            match item {
                nsh_core::MountItem::ViewProps { id, props }
                    if *id == radio && props.focusable == Some(true) =>
                {
                    observed.lock().unwrap().push("eligible")
                }
                nsh_core::MountItem::Command {
                    id,
                    command: nsh_core::Command::Focus,
                } if *id == radio => observed.lock().unwrap().push("focus"),
                _ => {}
            }
        }
    });
    app.press("role:radio[评 2 星]")?;
    let order = order.lock().unwrap();
    let eligible = order
        .iter()
        .position(|step| *step == "eligible")
        .ok_or_else(|| format!("no focus stop: {order:?}"))?;
    let focus = order
        .iter()
        .position(|step| *step == "focus")
        .ok_or_else(|| format!("no focus command: {order:?}"))?;
    assert!(
        eligible < focus,
        "macOS rejects focusing a view before it becomes a stop: {order:?}"
    );
    Ok(())
}
