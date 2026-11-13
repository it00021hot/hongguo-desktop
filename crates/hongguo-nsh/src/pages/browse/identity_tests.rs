use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn IdentityPaging() -> impl IntoElement {
    let query = signal(super::model::Query {
        offset: 18,
        session: "old-session".into(),
        identity: 0,
        previous: vec![hongguo_api::rank::RankItem {
            series_id: "old-account".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let feed: super::Feed = resource(
        || Some(()),
        |_, _| {
            std::future::ready(Ok(super::model::LoadedPage {
                scope: super::model::Query::default().scope(),
                page: hongguo_api::rank::RankPage {
                    items: vec![hongguo_api::rank::RankItem {
                        series_id: "fresh-account".into(),
                        ..Default::default()
                    }],
                    next_offset: 18,
                    has_more: true,
                    session_id: "new-session".into(),
                    ..Default::default()
                },
            }))
        },
    );
    view()
        .child(text(move || {
            if !feed.loading() && feed.with(|page| page.is_some()) {
                "fresh account response ready"
            } else {
                "loading response"
            }
        }))
        .child(
            crate::ui::button("Load next account page", crate::ui::ButtonVariant::Quiet)
                .on_press(move || super::next_page(query, feed, 1)),
        )
        .child(text(move || {
            format!(
                "query:{}:{}:{}",
                query.with(|q| q.identity),
                query.with(|q| q.offset),
                query.with(|q| q
                    .previous
                    .iter()
                    .map(|item| item.series_id.as_str())
                    .collect::<Vec<_>>()
                    .join(","))
            )
        }))
}

#[test]
fn a_fresh_identity_can_advance_even_when_its_first_cursor_is_not_above_the_previous_account_offset()
-> Result<(), String> {
    let mut app = Headless::start(EngineConfig::new(Root::of::<IdentityPaging>())).unwrap();
    app.wait_for("Load next account page")?;
    app.wait_for("text:fresh account response ready")?;
    app.press("Load next account page")?;
    app.wait_for("text:query:1:18:fresh-account")?;
    Ok(())
}
