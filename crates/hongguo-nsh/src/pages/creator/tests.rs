use super::*;
use nsh_engine::{Clock, EngineConfig, Environment, Root, Size};
use nsh_testing::Headless;
use std::time::{Duration, SystemTime};

#[component]
fn Creator() -> impl IntoElement {
    let nav = Navigation::new();
    nav.push(Route::Creator("offline-creator-main".into()));
    view()
        .flex_1()
        .child(ui::stack().child(super::preview(nav, "offline-creator-main".into())))
        .child(text(move || {
            format!(
                "route:{:?}",
                nav.routes().with(|routes| routes.last().cloned())
            )
        }))
        .child(text(move || {
            nav.routes().with(|routes| {
                format!(
                    "detail:{}",
                    routes.last().and_then(Route::detail_id).unwrap_or_default()
                )
            })
        }))
}

fn start(root: Root) -> Headless {
    Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(1280.0), px(840.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            root.tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        )
    })
    .unwrap()
}

#[test]
fn profile_works_and_a_work_detail_share_the_actual_series_identifier() -> Result<(), String> {
    let mut app = start(Root::of::<Creator>());
    app.wait_for("id:creator-profile >> text:离线创作者")?;
    app.wait_for("text:Works · 10")?;
    assert_eq!(app.screen().count("id:creator-works >> role:button")?, 10);
    app.press("id:creator-works >> role:button[装穷提亲，娶到全家嫌弃的她]")?;
    app.wait_for("text:detail:7701328280862854168")?;
    Ok(())
}

#[component]
fn FailedProfile() -> impl IntoElement {
    let nav = Navigation::new();
    let profile: Profile = resource(
        || Some(()),
        |_, _| {
            std::future::ready(Err(hongguo_common::AppError::Media(
                "offline profile failure".into(),
            )
            .into()))
        },
    );
    let works: Works = resource(
        || Some(()),
        |_, _| std::future::ready(Ok(fixtures::works())),
    );
    ui::stack().child(
        ui::page(nav, || tr("creator.title")).child(presentation::content(
            nav,
            profile,
            works,
            |_| {},
            |_| {},
            || {},
        )),
    )
}

#[test]
fn a_profile_failure_does_not_suppress_successful_works() -> Result<(), String> {
    let mut app = start(Root::of::<FailedProfile>());
    app.wait_for("id:creator-works")?;
    assert!(!app.screen().matches("id:creator-profile")?);
    assert_eq!(app.screen().count("id:creator-works >> role:button")?, 10);
    Ok(())
}

#[component]
fn RetryWorks() -> impl IntoElement {
    let nav = Navigation::new();
    let profile: Profile = resource(
        || Some(()),
        |_, _| std::future::ready(Ok(fixtures::profile())),
    );
    let calls = stored(0_u8);
    let works: Works = resource(
        || Some(()),
        move |_, _| {
            let first = calls.with(|calls| {
                *calls += 1;
                *calls == 1
            });
            std::future::ready(if first {
                Err(hongguo_common::AppError::Media("offline works failure".into()).into())
            } else {
                Ok(fixtures::works())
            })
        },
    );
    ui::stack().child(
        ui::page(nav, || tr("creator.title")).child(presentation::content(
            nav,
            profile,
            works,
            |_| {},
            |_| {},
            || {},
        )),
    )
}

#[test]
fn retrying_works_keeps_the_profile_and_recovers_the_grid() -> Result<(), String> {
    let mut app = start(Root::of::<RetryWorks>());
    app.wait_for("text:Failed to load homepage")?;
    assert!(app.screen().matches("id:creator-profile")?);
    app.press("role:button[Retry]")?;
    app.wait_for("id:creator-works")?;
    assert!(!app.shows("Failed to load homepage"));
    Ok(())
}

#[component]
fn PagedWorks() -> impl IntoElement {
    let nav = Navigation::new();
    let profile: Profile = resource(
        || Some(()),
        |_, _| std::future::ready(Ok(fixtures::profile())),
    );
    let query = signal(Query::default());
    let works: Works = resource(
        move || Some(query.get()),
        |query, _| async move {
            timer(Duration::from_millis(250)).await;
            let mut page = fixtures::works();
            if query.cursor.is_empty() {
                page.works.truncate(5);
                page.cursor = "opaque%2F+next".into();
                page.has_more = true;
            } else {
                page.works = page.works.into_iter().skip(4).collect();
                page.total = 0;
            }
            Ok(query.finish(page))
        },
    );
    ui::stack().child(
        ui::page(nav, || tr("creator.title")).child(presentation::content(
            nav,
            profile,
            works,
            |_| {},
            |_| {},
            move || next_page(query, works, 0),
        )),
    )
}

#[test]
fn real_parent_scroll_load_more_appends_without_duplicates_or_losing_total() -> Result<(), String> {
    let mut app = start(Root::of::<PagedWorks>());
    app.wait_for("id:creator-profile")?;
    app.run_for(Duration::from_millis(1000))?;
    app.wait_for("id:creator-works >> role:button[长街烟火]")?;
    assert_eq!(app.screen().count("id:creator-works >> role:button")?, 10);
    assert!(app.shows("Works · 10"));
    Ok(())
}
