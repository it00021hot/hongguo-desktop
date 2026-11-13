use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Environment, Points, Root, Size};
use nsh_testing::Headless;
use std::time::{Duration, SystemTime};

#[component]
fn Browse() -> impl IntoElement {
    let nav = crate::navigation::Navigation::new();
    nav.push(crate::navigation::Route::Browse);
    view()
        .flex_1()
        .child(crate::ui::stack().child(super::preview(nav)))
        .child(text(move || {
            format!(
                "route:{:?}",
                nav.routes().with(|routes| routes.last().cloned())
            )
        }))
}

fn start() -> Headless {
    start_root(Root::of::<Browse>())
}

fn start_root(root: Root) -> Headless {
    let environment = Environment {
        os: nsh_core::platform::Os::MacOs,
        ..Environment::default()
    };
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(Points(1280.0), Points(840.0)),
        environment,
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            root.tokens(nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    app.wait_for("id:browse-query").unwrap();
    app
}

#[test]
fn suggestions_wait_for_a_stable_two_character_query_and_offer_only_eight_rows()
-> Result<(), String> {
    let mut app = start();
    app.wait_for("id:browse-query")?;
    app.type_text("id:browse-query", "装")?;
    app.run_for(Duration::from_millis(400))?;
    assert!(!app.screen().matches("id:browse-suggestions")?);
    app.type_text("id:browse-query", "穷")?;
    app.run_for(Duration::from_millis(299))?;
    assert!(!app.screen().matches("id:browse-suggestions")?);
    app.run_for(Duration::from_millis(1))?;
    app.wait_for("id:browse-suggestions")?;
    assert_eq!(
        app.screen().count("id:browse-suggestions >> role:option")?,
        8
    );
    Ok(())
}

#[test]
fn arrows_wrap_only_visible_options_and_enter_opens_the_selected_series() -> Result<(), String> {
    let mut app = start();
    app.type_text("id:browse-query", "装穷")?;
    app.run_for(Duration::from_millis(300))?;
    app.wait_for("id:browse-suggestions")?;
    assert!(
        app.screen()
            .selected("role:option[装穷提亲，娶到全家嫌弃的她]")?
    );
    app.press_key("up")?;
    assert!(app.screen().selected("role:option[装穷之后]")?);
    app.press_key("down")?;
    app.press_key("down")?;
    assert!(app.screen().selected("role:option[装穷逆袭]")?);
    app.press_key("up")?;
    app.press_key("enter")?;
    app.wait_for("text:route:Some(Detail(\"7701328280862854168\"))")?;
    app.wait_for_gone("id:browse-suggestions", Duration::from_secs(1))?;
    Ok(())
}

#[test]
fn a_word_option_submits_search_and_exit_restores_filters() -> Result<(), String> {
    let mut app = start();
    app.type_text("id:browse-query", "装穷")?;
    app.run_for(Duration::from_millis(300))?;
    app.wait_for("id:browse-suggestions")?;
    app.press("role:option[装穷逆袭]")?;
    app.wait_for("role:button[Exit search]")?;
    assert!(
        !app.shows("10 titles"),
        "browse totals must not leak into search mode"
    );
    assert_eq!(
        app.screen().input_text("id:browse-query")?,
        Some("装穷逆袭")
    );
    assert!(!app.screen().matches("role:button[Filters]")?);
    app.press("role:button[Exit search]")?;
    app.wait_for("role:button[Filters]")?;
    assert_eq!(app.screen().input_text("id:browse-query")?, Some(""));
    Ok(())
}

#[test]
fn escape_and_leaving_the_input_dismiss_without_submitting() -> Result<(), String> {
    let mut app = start();
    app.type_text("id:browse-query", "装穷")?;
    app.run_for(Duration::from_millis(300))?;
    app.wait_for("id:browse-suggestions")?;
    app.press_key("escape")?;
    app.wait_for_gone("id:browse-suggestions", Duration::from_secs(1))?;
    assert!(!app.screen().matches("role:button[Exit search]")?);
    app.type_text("id:browse-query", "的")?;
    app.run_for(Duration::from_millis(300))?;
    app.wait_for("id:browse-suggestions")?;
    app.press_key("tab")?;
    app.wait_for_gone("id:browse-suggestions", Duration::from_secs(1))?;
    assert!(!app.screen().matches("role:button[Exit search]")?);
    Ok(())
}

#[component]
fn SlowSuggestions() -> impl IntoElement {
    let draft = signal(String::new());
    let result = signal(String::new());
    let search = super::search::Search::new(
        draft,
        || Some(0),
        |query| async move {
            timer(Duration::from_millis(if query == "装穷" {
                600
            } else {
                50
            }))
            .await;
            Ok(super::suggestion_fixtures::suggestions(&query))
        },
    );
    view()
        .flex_1()
        .child(super::search::field(
            search,
            move || result.set(search.term()),
            move |item| result.set(item.word),
        ))
        .child(
            crate::ui::button("Begin composition", crate::ui::ButtonVariant::Secondary)
                .on_press(move || search.composing.set(true)),
        )
        .child(
            crate::ui::button("End composition", crate::ui::ButtonVariant::Secondary).on_press(
                move || {
                    search.composing.set(false);
                    // This fixture button is outside the popup; native IME ends inside its input.
                    search.focus(true);
                },
            ),
        )
        .child(text(move || format!("submitted:{}", result.get())))
}

#[test]
fn late_suggestions_cannot_replace_the_current_query() -> Result<(), String> {
    let mut app = start_root(Root::of::<SlowSuggestions>());
    app.type_text("id:browse-query", "装穷")?;
    app.run_for(Duration::from_millis(301))?;
    app.type_text("id:browse-query", "总")?;
    app.run_for(Duration::from_millis(350))?;
    app.wait_for("role:option[装穷总裁]")?;
    app.run_for(Duration::from_secs(1))?;
    assert_eq!(
        app.screen().count("id:browse-suggestions >> role:option")?,
        1
    );
    assert!(!app.screen().matches("role:option[装穷逆袭]")?);
    Ok(())
}

#[test]
fn composing_input_does_not_offer_or_pick_suggestions_or_submit() -> Result<(), String> {
    let mut app = start_root(Root::of::<SlowSuggestions>());
    app.type_text("id:browse-query", "装穷总")?;
    app.run_for(Duration::from_millis(350))?;
    app.wait_for("id:browse-suggestions")?;
    app.press("role:button[Begin composition]")?;
    app.run_for(Duration::from_secs(1))?;
    assert!(!app.screen().matches("id:browse-suggestions")?);
    app.submit("id:browse-query")?;
    assert!(app.screen().matches("text:submitted:")?);
    app.press("role:button[End composition]")?;
    app.run_for(Duration::from_millis(350))?;
    app.wait_for("id:browse-suggestions")?;
    app.press_key("enter")?;
    app.wait_for("text:submitted:装穷总裁")?;
    Ok(())
}

#[test]
fn selected_filters_are_counted_and_pressing_the_same_choice_clears_it() -> Result<(), String> {
    let mut app = start();
    app.press("role:button[真人剧]")?;
    app.wait_for("role:button[Filters] >> text:1")?;
    app.press("role:button[逆袭]")?;
    app.wait_for("role:button[Filters] >> text:2")?;
    app.press("role:button[逆袭]")?;
    app.wait_for("role:button[Filters] >> text:1")?;
    app.press("role:button[真人剧]")?;
    assert!(!app.screen().matches("role:button[Filters] >> text:1")?);
    Ok(())
}

#[component]
fn SlowResults() -> impl IntoElement {
    use super::model::{LoadedPage, Query};
    use hongguo_api::rank::{RankItem, RankPage};
    let nav = crate::navigation::Navigation::new();
    nav.push(crate::navigation::Route::Browse);
    let query = signal(Query::default());
    let feed: super::Feed = resource(
        move || Some(query.get()),
        |query, _| async move {
            let scope = query.scope();
            if !query.term.is_empty() {
                timer(Duration::from_millis(250)).await;
            }
            let mut items = query.previous;
            let total = if query.term.is_empty() { 1 } else { 12 };
            let next = (query.offset + 6).min(total);
            items.extend((query.offset..next).map(|index| RankItem {
                series_id: format!("{}-{index}", query.term),
                title: if query.term.is_empty() {
                    "Original result".into()
                } else {
                    format!("New result {}", index + 1)
                },
                ..Default::default()
            }));
            Ok(LoadedPage {
                scope,
                page: RankPage {
                    items,
                    has_more: next < total,
                    next_offset: next,
                    ..Default::default()
                },
            })
        },
    );
    let panel = resource(|| Some(()), |_, _| std::future::ready(Ok(Vec::new())));
    let search = super::search::Search::new(
        signal(String::new()),
        || None,
        |_| std::future::ready(Ok(Vec::new())),
    );
    crate::ui::stack().child(super::presentation::content(
        nav,
        search,
        query,
        panel,
        feed,
        computed(Default::default),
        move || super::next_page(query, feed, 0),
    ))
}

#[test]
fn changing_search_owns_its_results_and_shows_skeleton_until_the_first_page() -> Result<(), String>
{
    let mut app = start_root(Root::of::<SlowResults>());
    app.wait_for("role:button[Original result]")?;
    app.type_text("id:browse-query", "next")?;
    app.submit("id:browse-query")?;
    assert!(!app.screen().matches("role:button[Original result]")?);
    assert!(app.screen().matches("id:browse-loading")?);
    assert!(!app.screen().enabled("role:button[Search]")?);
    app.run_for(Duration::from_millis(250))?;
    app.wait_for("role:button[New result 1]")?;
    assert!(!app.screen().matches("id:browse-loading")?);
    Ok(())
}

#[test]
fn sparse_search_pages_append_automatically_and_stop_at_the_real_end() -> Result<(), String> {
    let mut app = start_root(Root::of::<SlowResults>());
    app.type_text("id:browse-query", "next")?;
    app.submit("id:browse-query")?;
    app.run_for(Duration::from_millis(750))?;
    app.wait_for("role:button[New result 12]")?;
    assert_eq!(app.screen().count("id:browse-results >> role:button")?, 12);
    assert!(!app.screen().matches("role:button[Load more]")?);
    assert!(!app.screen().matches("id:browse-loading")?);
    app.run_for(Duration::from_secs(2))?;
    assert_eq!(app.screen().count("id:browse-results >> role:button")?, 12);
    Ok(())
}
