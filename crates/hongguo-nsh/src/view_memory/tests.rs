use super::*;
use crate::ui::{ButtonVariant, button};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[derive(Clone, Default, PartialEq)]
struct Query {
    term: String,
    page: usize,
}

#[component]
fn Form() -> impl IntoElement {
    let memory = scope("fixture");
    let count = memory.signal("count", 0_u32);
    let query = memory.projected("query", Query::default(), |query| Query {
        term: query.term.clone(),
        ..Default::default()
    });
    view()
        .child(text(move || format!("count:{}", count.get())))
        .child(text(move || {
            let query = query.get();
            format!("query:{}/{}", query.term, query.page)
        }))
        .child(button("Increment", ButtonVariant::Quiet).on_press(move || {
            count.update(|count| *count += 1);
        }))
        .child(
            button("Edit query", ButtonVariant::Quiet).on_press(move || {
                query.set(Query {
                    term: "saved".into(),
                    page: 7,
                });
            }),
        )
}

#[component]
fn Window() -> impl IntoElement {
    provide();
    let open = signal(true);
    view()
        .child(button("Hide form", ButtonVariant::Quiet).on_press(move || open.set(false)))
        .child(button("Show form", ButtonVariant::Quiet).on_press(move || open.set(true)))
        .child(when(open, || Form::element(())))
}

fn start() -> Result<Headless, String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<Window>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("text:count:0")?;
    Ok(app)
}

#[test]
fn remount_restores_values_without_reusing_the_disposed_signal_owner() -> Result<(), String> {
    let mut app = start()?;
    app.press("Increment")?;
    app.wait_for("text:count:1")?;
    app.press("Hide form")?;
    app.wait_for_gone("Increment", Duration::from_secs(1))?;
    app.press("Show form")?;
    app.wait_for("text:count:1")?;
    app.press("Increment")?;
    app.wait_for("text:count:2")?;
    let mut independent_window = start()?;
    independent_window.wait_for("text:count:0")?;
    Ok(())
}

#[test]
fn query_snapshots_restore_filters_and_discard_old_pagination_payloads() -> Result<(), String> {
    let mut app = start()?;
    app.press("Edit query")?;
    app.wait_for("text:query:saved/7")?;
    app.press("Hide form")?;
    app.wait_for_gone("Edit query", Duration::from_secs(1))?;
    app.press("Show form")?;
    app.wait_for("text:query:saved/0")?;
    Ok(())
}
