use super::*;
use crate::library::{
    feed,
    model::{Entry, Page, Query, merge_page},
    toolbar,
};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::time::Duration;

#[component]
fn Reservations() -> impl IntoElement {
    let logged_in = signal(false);
    let identity = signal(7_u64);
    let reads = signal(Vec::<String>::new());
    let query = signal(Query {
        online: true,
        ..Default::default()
    });
    let search = signal(String::new());
    let context = feed::Context {
        logged_in: computed(move || logged_in.get()),
        active: computed(|| true),
        identity: computed(move || identity.get()),
        revision: computed(|| 0),
        search: None,
    };
    let result = feed::load(query, context, move |query| {
        let account = query.identity.unwrap();
        reads.update(|reads| {
            reads.push(format!(
                "{account}/{}/{}",
                query.offset,
                query.previous.len()
            ))
        });
        std::future::ready(Ok(Page {
            items: merge_page(
                query.previous,
                vec![Entry {
                    key: format!("{account}-{}", query.offset),
                    title: format!("private-{account}-{}", query.offset),
                    ..Default::default()
                }],
            ),
            has_more: query.offset == 0,
            offset: query.offset + 1,
            ..Default::default()
        }))
    });
    let items = feed::current_items(result, query, context);
    let toolbar = toolbar::build(toolbar::Toolbar {
        kind: Kind::Reservations,
        query,
        search,
        category: signal(0),
        online: signal(true),
        busy: signal(false),
        managing: signal(false),
        selected: signal(Vec::new()),
        counts: computed(|| None),
        logged_in: context.logged_in,
    });
    view()
        .flex_1()
        .child(
            ui::button("Login seven", ui::ButtonVariant::Quiet)
                .on_press(move || logged_in.set(true)),
        )
        .child(
            ui::button("Logout", ui::ButtonVariant::Quiet).on_press(move || logged_in.set(false)),
        )
        .child(
            ui::button("Switch account", ui::ButtonVariant::Quiet)
                .on_press(move || identity.set(9)),
        )
        .child(text(move || format!("reads:{}", reads.get().join(","))))
        .child(frame(
            Kind::Reservations,
            toolbar,
            context.logged_in,
            Navigation::new(),
            move || {
                view()
                    .child(text(move || {
                        items
                            .get()
                            .into_iter()
                            .map(|item| item.title)
                            .collect::<Vec<_>>()
                            .join(",")
                    }))
                    .child(
                        ui::button("More", ui::ButtonVariant::Quiet)
                            .on_press(move || feed::advance(result, query, context)),
                    )
            },
        ))
}

#[test]
fn anonymous_reservations_keep_filters_and_never_load_or_reveal_account_rows() -> Result<(), String>
{
    let mut app = Headless::start(EngineConfig::new(
        Root::of::<Reservations>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    ))
    .map_err(|error| error.to_string())?;
    app.wait_for("Sign in to see your reservations")?;
    app.wait_for("Search reservations")?;
    app.wait_for("role:button[Online]")?;
    app.wait_for("role:button[Upcoming]")?;
    app.wait_for("text:reads:")?;
    assert!(!app.screen().matches("role:button[Manage]")?);
    app.press("role:button[Upcoming]")?;
    app.type_text("Search reservations", "kept")?;
    app.wait_for("text:reads:")?;
    app.press("Login seven")?;
    app.wait_for("private-7-0")?;
    app.wait_for("role:button[Manage]")?;
    app.press("More")?;
    app.wait_for("private-7-0,private-7-1")?;
    app.press("Logout")?;
    app.wait_for("Sign in to see your reservations")?;
    app.wait_for_gone("private-7-0,private-7-1", Duration::from_secs(1))?;
    app.press("Switch account")?;
    app.press("Login seven")?;
    app.wait_for("private-9-0")?;
    app.wait_for("text:reads:7/0/0,7/1/1,9/0/0")?;
    assert!(!app.screen().shows("private-7-0"));
    Ok(())
}
