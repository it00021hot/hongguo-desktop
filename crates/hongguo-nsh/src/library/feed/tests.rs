use super::*;
use crate::ui;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn SearchPages() -> impl IntoElement {
    let search = signal(String::new());
    let query = signal(Query {
        online: true,
        ..Default::default()
    });
    let reads = signal(Vec::<i64>::new());
    let fail = signal(false);
    let context = Context {
        logged_in: computed(|| true),
        active: computed(|| true),
        identity: computed(|| 7),
        revision: computed(|| 0),
        search: Some(search),
    };
    let feed = load(query, context, move |query| {
        reads.update(|reads| reads.push(query.offset));
        if query.offset == 1 && fail.get_untracked() {
            return std::future::ready(Err(hongguo_common::AppError::Rejected(
                "fixture page failure".into(),
            )
            .into()));
        }
        let incoming = match query.offset {
            0 => vec![Entry {
                key: "first".into(),
                title: "First".into(),
                ..Default::default()
            }],
            1 => Vec::new(),
            _ => vec![Entry {
                key: "last".into(),
                title: "Needle".into(),
                ..Default::default()
            }],
        };
        std::future::ready(Ok(Page {
            items: crate::library::model::merge_page(query.previous, incoming),
            offset: query.offset + 1,
            session: "session".into(),
            has_more: query.offset < 2,
            ..Default::default()
        }))
    });
    let items = current_items(feed, query, context);
    view()
        .flex_1()
        .child(ui::library_search(search, "Search reservations"))
        .child(view().test_id("reservation-matches").child(text(move || {
            items
                .get()
                .into_iter()
                .filter(|item| item.title.contains(&search.get()))
                .map(|item| item.title)
                .collect::<Vec<_>>()
                .join(",")
        })))
        .child(text(move || format!("reads:{:?}", reads.get())))
        .child(text(move || {
            if feed.error().is_some() {
                "Page failed"
            } else {
                "Page ready"
            }
        }))
        .child(
            ui::button("Fail next page", ui::ButtonVariant::Quiet).on_press(move || fail.set(true)),
        )
        .child(
            ui::button("Retry", ui::ButtonVariant::Quiet).on_press(move || {
                fail.set(false);
                feed.refetch();
            }),
        )
        .child(
            ui::button("More", ui::ButtonVariant::Quiet)
                .on_press(move || advance(feed, query, context)),
        )
}

#[test]
fn search_stops_on_page_error_until_an_explicit_retry() -> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(
        Root::of::<SearchPages>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    ))
    .map_err(|error| error.to_string())?;
    app.wait_for("First")?;
    app.press("Fail next page")?;
    app.type_text("Search reservations", "Needle")?;
    app.wait_for("Page failed")?;
    app.wait_for("text:reads:[0, 1]")?;
    app.press("More")?;
    app.wait_for("text:reads:[0, 1]")?;
    app.press("Retry")?;
    app.wait_for("id:reservation-matches >> text:Needle")?;
    app.wait_for("text:reads:[0, 1, 1, 2]")?;
    Ok(())
}

#[test]
fn reservation_search_reaches_later_pages_and_skips_empty_intermediate_pages() -> Result<(), String>
{
    let mut app = Headless::start(EngineConfig::new(
        Root::of::<SearchPages>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
    ))
    .map_err(|error| error.to_string())?;
    app.wait_for("First")?;
    app.wait_for("text:reads:[0]")?;
    app.type_text("Search reservations", "Needle")?;
    app.wait_for("id:reservation-matches >> text:Needle")?;
    app.wait_for("text:reads:[0, 1, 2]")?;
    app.press("More")?;
    app.wait_for("text:reads:[0, 1, 2]")?;
    Ok(())
}
