use super::model::{Feed, Item};
use nsh_core::prelude::*;

/// The sole scrolling owner. Header/footer participate in layout and row recycling.
pub(super) fn list(
    feed: Signal<Feed>,
    header: View,
    footer: View,
    empty: impl IntoElement,
    id: &'static str,
    row: impl Fn(ReadSignal<Item>) -> AnyElement + 'static,
) -> View {
    let row = stored(Box::new(row) as Box<dyn Fn(ReadSignal<Item>) -> AnyElement>);
    view().flex_1().min_h_0().child(
        virtual_list(
            move || feed.with(|feed| feed.items.len()),
            move |at| {
                let item = computed(move || feed.with(|feed| feed.items.get(at.index()).cloned()));
                view().child(each(
                    move || item.get().into_iter().collect::<Vec<_>>(),
                    |item: &Item| item.id().to_owned(),
                    move |item, _| row.with(|row| row(item)),
                ))
            },
        )
        .test_id(id)
        .keys(move || {
            feed.with(|feed| {
                feed.items
                    .iter()
                    .map(|item| item.id().to_owned())
                    .collect::<Vec<_>>()
            })
        })
        .header(header)
        .footer(footer)
        .empty(empty)
        .flex_1(),
    )
}
