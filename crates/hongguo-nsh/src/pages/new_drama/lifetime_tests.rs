use super::*;
use hongguo_api::{
    calendar::{CalendarItem, CalendarPage},
    rank::{RankItem, RankPage},
};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Clone, Copy)]
struct TestState {
    nav: Navigation,
    identity: Signal<u64>,
    delayed: Signal<bool>,
    pending: Stored<Option<oneshot::Sender<()>>>,
}

impl TestState {
    fn gate(self) -> Option<oneshot::Receiver<()>> {
        self.delayed.get_untracked().then(|| {
            let (send, receive) = oneshot::channel();
            self.pending.with(|pending| *pending = Some(send));
            receive
        })
    }
}

#[component]
fn NewPage() -> impl IntoElement {
    let state = use_context::<TestState>().unwrap();
    let (mode, recommend_active, calendar_active) = controls(state.nav);
    let (recommend_query, recommend_feed) = recommend::data(
        recommend_active,
        move || state.identity.get(),
        move |query| {
            let gate = state.gate();
            async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                Ok(model::finish_recommendation(
                    RankPage {
                        items: vec![RankItem {
                            series_id: format!("{}:{}", query.identity, query.offset),
                            ..Default::default()
                        }],
                        session_id: format!("identity:{}", query.identity),
                        ..Default::default()
                    },
                    query,
                ))
            }
        },
    );
    let (selected, calendar_query, calendar_feed) = calendar::data(
        calendar_active,
        move || state.identity.get(),
        move |query| {
            let gate = state.gate();
            async move {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                let mut items = query.previous;
                items.push(CalendarItem {
                    series_id: format!("{}:{}:{}", query.identity, query.date, query.offset),
                    ..Default::default()
                });
                Ok(CalendarPage {
                    items,
                    has_more: true,
                    next_offset: query.offset + 1,
                    session_id: format!("identity:{}", query.identity),
                    ..Default::default()
                })
            }
        },
    );
    view()
        .child(text(move || {
            format!("mode:{}", usize::from(mode.get() == Mode::Calendar))
        }))
        .child(text(move || {
            format!(
                "recommend rows:{} cursor:{}",
                recommend_feed.with(|page| page.map_or(0, |page| page.items.len())),
                recommend_query.with(|query| query.offset)
            )
        }))
        .child(text(move || {
            format!(
                "calendar rows:{} cursor:{} date:{}",
                calendar_feed.with(|page| page.map_or(0, |page| page.items.len())),
                calendar_query.with(|query| query.offset),
                selected.get()
            )
        }))
        .child(text(move || {
            format!(
                "recommend session:{}",
                recommend_feed
                    .get()
                    .map(|page| page.session_id)
                    .unwrap_or_default()
            )
        }))
        .child(text(move || {
            format!(
                "calendar session:{}",
                calendar_feed
                    .get()
                    .map(|page| page.session_id)
                    .unwrap_or_default()
            )
        }))
        .child(
            ui::button("Recommendations", ui::ButtonVariant::Quiet)
                .on_press(move || mode.set(Mode::Recommend)),
        )
        .child(
            ui::button("Calendar", ui::ButtonVariant::Quiet)
                .on_press(move || mode.set(Mode::Calendar)),
        )
        .child(
            ui::button("Next day", ui::ButtonVariant::Quiet)
                .on_press(move || selected.set("20261114".into())),
        )
        .child(
            ui::button("More recommendations", ui::ButtonVariant::Quiet).on_press(move || {
                let page = recommend_feed.get().unwrap();
                recommend_query.update(|query| {
                    query.offset = page.next_offset;
                    query.previous = page.items;
                    query.session = page.session_id;
                });
            }),
        )
        .child(
            ui::button("More calendar", ui::ButtonVariant::Quiet).on_press(move || {
                let page = calendar_feed.get().unwrap();
                calendar_query.update(|query| {
                    query.offset = page.next_offset;
                    query.previous = page.items;
                    query.session = page.session_id;
                });
            }),
        )
}

#[component]
fn Window() -> impl IntoElement {
    crate::view_memory::provide();
    crate::view_memory::provide_models();
    let nav = Navigation::new();
    nav.push(Route::New);
    let state = TestState {
        nav,
        identity: signal(0),
        delayed: signal(false),
        pending: stored(None),
    };
    provide_context(state);
    view()
        .child(ui::button("Leave new", ui::ButtonVariant::Quiet).on_press(move || nav.back()))
        .child(
            ui::button("Return new", ui::ButtonVariant::Quiet)
                .on_press(move || nav.push(Route::New)),
        )
        .child(
            ui::button("Delay loads", ui::ButtonVariant::Quiet)
                .on_press(move || state.delayed.set(true)),
        )
        .child(
            ui::button("Release load", ui::ButtonVariant::Quiet).on_press(move || {
                if let Some(send) = state.pending.with(Option::take) {
                    let _ = send.send(());
                }
                state.delayed.set(false);
            }),
        )
        .child(
            ui::button("Change identity", ui::ButtonVariant::Quiet).on_press(move || {
                state.delayed.set(false);
                state.identity.update(|identity| *identity += 1);
            }),
        )
        .child(when(
            move || {
                nav.routes()
                    .with(|routes| routes.last() == Some(&Route::New))
            },
            || NewPage::element(()),
        ))
}

fn start() -> Result<Headless, String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<Window>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("text:recommend rows:1 cursor:0")?;
    Ok(app)
}

#[test]
fn loaded_recommendations_and_paging_survive_menu_disposal_during_a_delayed_refresh()
-> Result<(), String> {
    let mut app = start()?;
    app.press("More recommendations")?;
    app.wait_for("text:recommend rows:2 cursor:1")?;
    app.press("Leave new")?;
    app.wait_for_gone("Recommendations", Duration::from_secs(1))?;
    app.press("Delay loads")?;
    app.press("Return new")?;
    app.wait_for("text:recommend rows:2 cursor:1")?;
    app.press("Release load")?;
    app.wait_for("text:recommend rows:2 cursor:1")?;
    app.press("More recommendations")?;
    app.wait_for("text:recommend rows:3 cursor:2")?;
    Ok(())
}

#[test]
fn calendar_date_mode_and_rows_survive_remount_and_identity_resets_only_paging()
-> Result<(), String> {
    let mut app = start()?;
    app.press("Calendar")?;
    app.press("Next day")?;
    app.wait_for("text:calendar rows:1 cursor:0 date:20261114")?;
    app.press("More calendar")?;
    app.wait_for("text:calendar rows:2 cursor:1 date:20261114")?;
    app.press("Leave new")?;
    app.wait_for_gone("Calendar", Duration::from_secs(1))?;
    app.press("Delay loads")?;
    app.press("Return new")?;
    app.wait_for("text:mode:1")?;
    app.wait_for("text:calendar rows:2 cursor:1 date:20261114")?;
    app.press("Release load")?;
    app.press("Change identity")?;
    app.wait_for("text:calendar rows:1 cursor:0 date:20261114")?;
    app.wait_for("text:calendar session:identity:1")?;
    app.press("Recommendations")?;
    app.wait_for("text:recommend rows:1 cursor:0")?;
    app.wait_for("text:recommend session:identity:1")?;
    let mut another = start()?;
    another.wait_for("text:mode:0")?;
    another.wait_for("text:calendar rows:0 cursor:0 date:")?;
    Ok(())
}

#[component]
fn ScrollWindow() -> impl IntoElement {
    crate::view_memory::provide();
    let shown = signal(true);
    let audience = signal(2_i64);
    view()
        .flex_1()
        .child(ui::button("Hide grid", ui::ButtonVariant::Quiet).on_press(move || shown.set(false)))
        .child(ui::button("Show grid", ui::ButtonVariant::Quiet).on_press(move || shown.set(true)))
        .child(ui::button("Male", ui::ButtonVariant::Quiet).on_press(move || audience.set(1)))
        .child(ui::button("All", ui::ButtonVariant::Quiet).on_press(move || audience.set(2)))
        .child(when(shown, move || {
            ui::new_drama_grid(
                || {
                    (0..80)
                        .map(|index| ui::NewDramaCard {
                            id: index.to_string(),
                            title: format!("Drama {index}"),
                            ..Default::default()
                        })
                        .collect::<Vec<_>>()
                },
                true,
                |_| {},
                || {},
                audience,
            )
        }))
}

#[test]
fn actual_recommendation_list_restores_each_audience_after_menu_remount() -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<ScrollWindow>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(600.0), px(400.0));
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("id:new-recommend-grid")?;
    app.scroll("id:new-recommend-grid", px(800.0))?;
    app.press("Male")?;
    app.wait(Duration::from_millis(32))?;
    assert!(app.scroll("id:new-recommend-grid", px(-1.0)).is_err());
    app.scroll("id:new-recommend-grid", px(400.0))?;
    app.press("All")?;
    app.wait(Duration::from_millis(32))?;
    app.scroll("id:new-recommend-grid", px(-800.0))?;
    assert!(app.scroll("id:new-recommend-grid", px(-1.0)).is_err());
    app.press("Male")?;
    app.wait(Duration::from_millis(32))?;
    app.press("Hide grid")?;
    app.wait_for_gone("id:new-recommend-grid", Duration::from_secs(1))?;
    app.press("Show grid")?;
    app.wait_for("id:new-recommend-grid")?;
    app.scroll("id:new-recommend-grid", px(-400.0))?;
    assert!(app.scroll("id:new-recommend-grid", px(-1.0)).is_err());
    Ok(())
}
