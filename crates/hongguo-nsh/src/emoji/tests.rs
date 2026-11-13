use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn Host() -> impl IntoElement {
    view().w(px(480.0)).h(px(700.0)).child(super::fixture())
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<Host>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(600.0), px(800.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("离线表情与附件验证")?;
    Ok(app)
}

#[test]
fn system_selection_keeps_the_panel_open_and_submits_the_original_code() -> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[表情]")?;
    app.wait_for("role:button[[偷笑]]")?;
    app.press("role:button[[偷笑]]")?;
    app.wait_for("role:button[[飞吻]]")?;
    app.press_key("escape")?;
    app.press("role:button[发送]")?;
    app.wait_for("已发送：你好[微笑][偷笑]")?;
    Ok(())
}

#[test]
fn rich_body_tab_reaches_the_real_neighboring_actions() -> Result<(), String> {
    let mut app = start()?;
    app.type_text("留下你的精彩评论吧", "键盘正文")?;
    app.press_key("shift+tab")?;
    app.press_key("space")?;
    app.press("role:button[发送]")?;
    app.wait_for("离线模拟：发送失败，草稿已保留")?;
    app.type_text("留下你的精彩评论吧", "表情")?;
    app.press_key("tab")?;
    app.press_key("space")?;
    app.wait_for("role:button[[偷笑]]")?;
    app.press_key("escape")?;
    Ok(())
}

#[test]
fn attachments_allow_image_only_send_and_a_failed_send_keeps_the_draft() -> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[发送]")?;
    app.press("role:button[图片]")?;
    app.wait_for("role:button[移除图片]")?;
    app.press("role:button[模拟发送失败]")?;
    app.press("role:button[发送]")?;
    app.wait_for("离线模拟：发送失败，草稿已保留")?;
    assert!(app.screen().matches("role:button[移除图片]")?);
    app.press("role:button[发送]")?;
    app.wait_for("已发送：（含图片）")?;
    Ok(())
}

#[test]
fn remote_search_and_collection_share_the_same_selected_attachment_flow() -> Result<(), String> {
    let mut app = start()?;
    app.press("role:button[表情]")?;
    app.press("role:button[搜索表情]")?;
    app.type_text("搜索表情包…", "笑")?;
    app.wait_for("role:button[测试表情0]")?;
    app.right_click("role:button[测试表情0]")?;
    app.wait_for("添加到表情")?;
    app.press("添加到表情")?;
    app.wait_for("已添加到我的表情")?;
    assert!(!app.screen().matches("role:menuitem[添加到表情]")?);
    app.press("role:button[自定义表情]")?;
    app.wait_for("role:button[上传自定义表情]")?;
    app.wait_for("role:button[测试表情0]")?;
    app.press("role:button[测试表情0]")?;
    app.press_key("escape")?;
    app.wait_for("role:button[移除图片]")?;
    Ok(())
}
