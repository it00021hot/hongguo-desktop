use super::*;
use nsh_engine::{EngineConfig, Environment, Root, Size};
use nsh_testing::Headless;

fn seed() -> DetailPrefill {
    DetailPrefill {
        series_id: "preview-upcoming".into(),
        title: "即将相遇的故事".into(),
        cover: crate::pages::fixtures::FRAME_PATH.into(),
        tags: vec!["恋爱".into()],
        desc: "来源页已经提供的剧情介绍。".into(),
    }
}

#[component]
fn Upcoming() -> impl IntoElement {
    let nav = Navigation::new();
    ui::stack().child(preview_upcoming(nav, seed()))
}

fn start() -> Headless {
    Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(1280.0), px(840.0)),
        ..EngineConfig::new(
            Root::of::<Upcoming>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        )
    })
    .unwrap()
}

#[test]
fn a_failed_seeded_resolution_shows_the_source_archive_and_reservation_instead_of_play()
-> Result<(), String> {
    let mut app = start();
    app.wait_for("text:即将相遇的故事")?;
    assert!(app.shows("来源页已经提供的剧情介绍。"));
    assert!(app.shows("Coming soon"));
    assert!(!app.screen().matches("role:button[Play from EP 1]")?);
    assert!(!app.screen().matches("role:button[Retry]")?);
    app.press("role:button[Reserve]")?;
    app.wait_for("role:button[Reserved]")?;
    assert!(!app.screen().matches("role:button[Retry]")?);
    app.press("role:button[Reserved]")?;
    app.wait_for("role:button[Reserve]")?;
    Ok(())
}

#[component]
fn NarrowDetail() -> impl IntoElement {
    let nav = Navigation::new();
    let series = hongguo_common::model::Series {
        series_id: "live".into(),
        title: "Released story".into(),
        episode_count: 1,
        episodes: vec![hongguo_common::model::Episode {
            vid_index: 1,
            ..Default::default()
        }],
        ..Default::default()
    };
    ui::stack().child(preview(nav, series, Default::default()))
}

#[test]
fn a_narrow_desktop_keeps_real_collect_and_like_actions_above_bottom_navigation()
-> Result<(), String> {
    let mut app = Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(480.0), px(840.0)),
        ..EngineConfig::new(
            Root::of::<NarrowDetail>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    app.wait_for("text:Released story")?;
    assert!(app.screen().matches("id:detail-mobile-actions")?);
    app.press("id:detail-mobile-actions >> role:button[Collect]")?;
    app.wait_for("id:detail-mobile-actions >> role:button[Collected]")?;
    app.press("id:detail-mobile-actions >> role:button[Like]")?;
    assert!(
        app.screen()
            .selected("id:detail-mobile-actions >> role:button[Like]")?
    );
    Ok(())
}

#[component]
fn CalendarSnapshot() -> impl IntoElement {
    let nav = Navigation::new();
    let item = hongguo_api::calendar::CalendarItem {
        series_id: "future-calendar".into(),
        title: "Calendar story".into(),
        category: "Category outside source tags".into(),
        rec_tags: vec!["Calendar tag A".into(), "Calendar tag B".into()],
        ..Default::default()
    };
    ui::stack().child(preview_upcoming(nav, DetailPrefill::from_calendar(&item)))
}

#[test]
fn calendar_snapshot_displays_the_same_rec_tags_passed_by_the_real_source_row() -> Result<(), String>
{
    let mut app = Headless::start(EngineConfig::new(
        Root::of::<CalendarSnapshot>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    ))
    .unwrap();
    app.wait_for("text:Calendar story")?;
    assert!(app.shows("Calendar tag A"));
    assert!(app.shows("Calendar tag B"));
    assert!(!app.shows("Category outside source tags"));
    Ok(())
}
