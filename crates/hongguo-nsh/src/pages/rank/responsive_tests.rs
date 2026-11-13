use super::*;
use nsh_core::platform::Os;
use nsh_engine::{EngineConfig, Root, Size};
use nsh_testing::Headless;

const SUB_LIST_COUNT: usize = 24;

#[component]
fn RankLayout() -> impl IntoElement {
    let nav = Navigation::new();
    nav.push(Route::Rank);
    let query = signal(Query::default());
    let mut page = crate::pages::fixtures::catalog().rank;
    let template = page.tabs[0].subs[0].clone();
    page.tabs[0].subs = (0..SUB_LIST_COUNT)
        .map(|index| hongguo_api::rank::RankSubList {
            id: format!("sub-{index}"),
            name: format!("分类 {index:02}"),
            ..template.clone()
        })
        .collect();
    let page = stored(page);
    let source: series_feed::Feed = resource(
        move || Some(page.get()),
        |page, _| std::future::ready(Ok(page)),
    );
    view()
        .flex_1()
        .child(presentation::content(nav, query, source, view().flex_1()))
        .child(text(move || format!("selected:{}", query.get().sub)))
}

fn start(os: Os, width: f32) -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<RankLayout>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = Size::new(px(width), px(360.0));
    config.environment.os = os;
    config.environment.reduce_motion = true;
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("id:rank-sub-lists")?;
    app.wait_for("role:button[分类 00]")?;
    Ok(app)
}

fn reach_last(app: &mut Headless) -> Result<(), String> {
    if app.scroll("id:rank-sub-lists", px(5000.0)).is_err() {
        app.scroll("id:rank-sub-lists", px(-5000.0))?;
        app.scroll("id:rank-sub-lists", px(5000.0))?;
    }
    Ok(())
}

#[test]
fn narrow_rank_sub_lists_scroll_to_real_final_and_first_options() -> Result<(), String> {
    for os in [Os::Android, Os::Ios, Os::MacOs] {
        let mut app = start(os, 393.0)?;
        app.scroll("id:rank-sub-lists", px(5000.0))?;
        app.press("role:button[分类 23]")?;
        app.wait_for("text:selected:sub-23")?;
        app.scroll("id:rank-sub-lists", px(-5000.0))?;
        app.press("role:button[分类 00]")?;
        app.wait_for("text:selected:sub-0")?;
    }
    Ok(())
}

#[test]
fn rank_sub_lists_switch_scroll_axis_at_the_breakpoint_without_losing_selection()
-> Result<(), String> {
    let mut app = start(Os::MacOs, 1280.0)?;
    app.scroll("id:rank-sub-lists", px(5000.0))?;
    app.press("role:button[分类 23]")?;
    app.wait_for("text:selected:sub-23")?;
    app.set_viewport(Size::new(px(393.0), px(360.0)))?;
    reach_last(&mut app)?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:button[分类 23]")?
            .pressed,
        Some(nsh_core::Checked::True)
    );
    app.press("role:button[分类 23]")?;
    app.wait_for("text:selected:sub-23")?;
    app.set_viewport(Size::new(px(1280.0), px(360.0)))?;
    reach_last(&mut app)?;
    assert_eq!(
        app.screen()
            .accessibility_state("role:button[分类 23]")?
            .pressed,
        Some(nsh_core::Checked::True)
    );
    Ok(())
}

#[test]
fn rank_sub_list_arrows_move_focus_without_selecting_until_activation() -> Result<(), String> {
    let mut app = start(Os::MacOs, 1280.0)?;
    app.press_key("tab")?;
    app.press_key("right")?;
    let second = app.screen().press_target("role:button[分类 01]")?.unwrap();
    assert!(
        app.screen()
            .commands()
            .iter()
            .rev()
            .find(|(_, command)| matches!(command, nsh_core::Command::Focus))
            .is_some_and(|(id, _)| *id == second),
        "right must move real focus to the next sublist"
    );
    app.wait_for("text:selected:sub-0")?;
    app.press_key("enter")?;
    app.wait_for("text:selected:sub-1")?;
    app.press_key("end")?;
    app.press_key("space")?;
    app.wait_for("text:selected:sub-23")?;
    app.press_key("right")?;
    app.press_key("enter")?;
    app.wait_for("text:selected:sub-0")?;
    app.press_key("end")?;
    app.press_key("home")?;
    app.press_key("enter")?;
    app.wait_for("text:selected:sub-0")?;
    app.press_key("tab")?;
    app.press_key("shift+tab")?;
    app.press_key("right")?;
    app.wait_for("text:selected:sub-0")?;
    app.press_key("enter")?;
    app.wait_for("text:selected:sub-1")?;
    app.press_key("tab")?;
    app.press_key("enter")?;
    app.wait_for("id:rank-month-filter")?;
    app.press_key("escape")?;
    app.wait_for_gone("id:rank-month-filter", std::time::Duration::from_secs(1))?;
    Ok(())
}

#[test]
fn rank_sub_list_focus_reveals_offscreen_rows_before_confirmation() -> Result<(), String> {
    for width in [1280.0, 393.0] {
        let mut app = start(Os::MacOs, width)?;
        app.press_key("tab")?;
        app.press_key("end")?;
        let revealed = app.screen().commands().iter().any(|(_, command)| {
            if let nsh_core::Command::ScrollTo { x, y, .. } = command {
                (if width < 768.0 { x.0 } else { y.0 }) > 0.0
            } else {
                false
            }
        });
        assert!(revealed, "End must reveal the actually focused final row");
        app.wait_for("text:selected:sub-0")?;
        app.press_key("home")?;
        assert!(
            app.screen()
                .commands()
                .iter()
                .rev()
                .find_map(|(_, command)| {
                    if let nsh_core::Command::ScrollTo { x, y, .. } = command {
                        Some(if width < 768.0 { x.0 } else { y.0 })
                    } else {
                        None
                    }
                })
                .is_some_and(|offset| offset == 0.0),
            "Home must reveal the actually focused first row"
        );
    }
    Ok(())
}
