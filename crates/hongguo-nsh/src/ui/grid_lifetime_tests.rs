use super::*;
use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root, Size};
use nsh_testing::Headless;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy)]
enum Kind {
    Browse,
    New,
}

fn host(kind: Kind, name: &'static str) -> View {
    test_log::watch(name);
    let loading = signal(false);
    let generation = signal(0_u32);
    let rows = computed(move || {
        if loading.get() {
            Vec::new()
        } else {
            (0..40)
                .map(|index| BrowseCard {
                    id: format!("{}-{index}", generation.get()),
                    title: format!("Result {}-{index}", generation.get()),
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        }
    });
    let list = match kind {
        Kind::Browse => when(
            move || !rows.with(Vec::is_empty),
            move || form(browse_grid(rows, |_| {}, || {})),
        )
        .into_element(),
        Kind::New => dynamic(move || {
            generation.get();
            new_drama_grid(
                move || {
                    rows.with(|rows| {
                        rows.iter()
                            .map(|row| NewDramaCard {
                                id: row.id.clone(),
                                title: row.title.clone(),
                                ..Default::default()
                            })
                            .collect::<Vec<_>>()
                    })
                },
                true,
                |_| {},
                || {},
                2_i64,
            )
        }),
    };
    view()
        .flex_1()
        .child(
            button("Replace query", ButtonVariant::Quiet).on_press(move || {
                batch(|| {
                    loading.set(true);
                    generation.update(|generation| *generation += 1);
                })
            }),
        )
        .child(button("Complete query", ButtonVariant::Quiet).on_press(move || loading.set(false)))
        .child(list)
}

#[component]
fn BrowseGridLifetime() -> impl IntoElement {
    host(Kind::Browse, "browse-grid-lifetime")
}
#[component]
fn NewGridLifetime() -> impl IntoElement {
    host(Kind::New, "new-grid-lifetime")
}

fn replace(root: Root, name: &'static str) -> Result<(), String> {
    let mut config = EngineConfig::new(
        root.tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
    );
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.viewport = Size::new(px(1280.0), px(840.0));
    config.clock = Clock::manual(SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("role:button[Result 0-0]")?;
    for generation in 1..=3 {
        app.press("Replace query")?;
        app.run_for(Duration::from_millis(16))?;
        assert!(!app.screen().matches("role:button[Result 0-0]")?);
        app.press("Complete query")?;
        app.wait_for(&format!("role:button[Result {generation}-0]"))?;
    }
    drop(app);
    let errors = test_log::take(name);
    assert!(errors.is_empty(), "virtual grid owner errors: {errors:?}");
    Ok(())
}

#[test]
fn browse_cells_stop_reading_the_old_query_when_the_grid_is_removed_and_refilled()
-> Result<(), String> {
    replace(Root::of::<BrowseGridLifetime>(), "browse-grid-lifetime")
}

#[test]
fn new_channel_replacement_disposes_its_cells_before_their_query_owner() -> Result<(), String> {
    replace(Root::of::<NewGridLifetime>(), "new-grid-lifetime")
}
