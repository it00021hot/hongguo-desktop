use hongguo_api::search::SuggestItem;

pub(super) fn suggestions(query: &str) -> Vec<SuggestItem> {
    let items: Vec<SuggestItem> = serde_json::from_str(include_str!(
        "../../../preview/fixtures/search-suggestions.json"
    ))
    .expect("valid offline search suggestions");
    items
        .into_iter()
        .filter(|item| item.word.contains(query.trim()))
        .map(|mut item| {
            if !item.cover.is_empty() {
                item.cover = crate::pages::fixtures::FRAME_PATH.into();
            }
            item
        })
        .collect()
}
