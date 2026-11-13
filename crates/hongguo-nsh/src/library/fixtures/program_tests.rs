use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[test]
fn offline_follow_and_collection_previews_render_programs_and_manage_the_same_three_entries()
-> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<crate::LayoutPreview>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("id:main-nav")?;
    for page in ["追更", "收藏"] {
        app.press(&format!("id:main-nav >> role:button[{page}]"))?;
        for title in ["装穷提亲，娶到全家嫌弃的她", "下一段故事", "归途有风"]
        {
            app.wait_for(title)?;
        }
        app.press("role:button[管理]")?;
        app.press("role:button[全选]")?;
        app.wait_for("已选 3 项")?;
        app.press("role:button[退出管理]")?;
    }
    Ok(())
}
