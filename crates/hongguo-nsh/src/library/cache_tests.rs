use crate::{
    bridge,
    events::SessionHandle,
    navigation::{Navigation, Route},
    ui,
};
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

pub(crate) fn window(
    panel: impl Fn(SessionHandle, Navigation) -> AnyElement + Copy + 'static,
) -> impl IntoElement {
    let _notifications = crate::notifications::install();
    let loaded = resource(
        || Some(()),
        |_, _| {
            bridge::request(crate::bootstrap::Session::for_test(
                Default::default(),
                Vec::new(),
            ))
        },
    );
    when(
        move || loaded.with(|loaded| loaded.is_some()),
        move || {
            crate::view_memory::provide();
            crate::view_memory::provide_models();
            let session = SessionHandle::new(loaded.get().unwrap());
            let nav = Navigation::new();
            let shown = signal(true);
            view()
                .child(
                    ui::button("Leave model", ui::ButtonVariant::Quiet)
                        .on_press(move || shown.set(false)),
                )
                .child(
                    ui::button("Return model", ui::ButtonVariant::Quiet)
                        .on_press(move || shown.set(true)),
                )
                .child(
                    ui::button("Change identity", ui::ButtonVariant::Quiet)
                        .on_press(move || session.notify_identity()),
                )
                .child(when(shown, move || panel(session, nav)))
        },
    )
}

#[component]
fn LibraryModels() -> impl IntoElement {
    window(|session, nav| {
        nav.push(Route::Collections);
        let model = super::data::load(session, nav, super::Kind::Collections);
        let managing = signal(false);
        view()
            .test_id("library-model")
            .child(text(move || {
                format!("query:{}:{}", model.query.get().offset, model.search.get())
            }))
            .child(text(move || {
                format!(
                    "active:{} identity:{}",
                    model.context.active.get(),
                    model.context.identity.get()
                )
            }))
            .child(text(move || {
                format!(
                    "rows:{}",
                    model
                        .result
                        .with(|page| page.map_or(0, |page| page.items.len()))
                )
            }))
            .child(text(move || format!("managing:{}", managing.get())))
            .child(
                ui::button("Seed library model", ui::ButtonVariant::Quiet).on_press(move || {
                    model.search.set("kept".into());
                    model.query.update(|query| query.offset = 7);
                    model.result.mutate(super::model::Page {
                        items: vec![super::model::Entry::default()],
                        ..Default::default()
                    });
                    managing.set(true);
                }),
            )
            .into_element()
    })
}

#[test]
fn library_data_survives_page_disposal_while_management_remains_local() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<LibraryModels>()))
        .map_err(|error| error.to_string())?;
    app.wait_for("id:library-model")?;
    app.press("Seed library model")?;
    app.wait_for("text:query:7:kept")?;
    app.wait_for("text:rows:1")?;
    app.wait_for("text:managing:true")?;
    app.press("Leave model")?;
    app.wait_for_gone("id:library-model", Duration::from_secs(1))?;
    app.press("Change identity")?;
    app.press("Return model")?;
    app.wait_for("text:query:7:kept")?;
    app.wait_for("text:rows:1")?;
    app.wait_for("text:managing:false")?;
    app.wait_for("text:active:true identity:1")?;
    Ok(())
}
