use super::model::Feed;
use nsh_core::prelude::*;

#[cfg(test)]
#[derive(Clone, PartialEq)]
pub(super) struct VisibleRows {
    pub first: usize,
    pub last: usize,
    pub key: String,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) struct ViewabilityProbe(pub Signal<Option<VisibleRows>>);

pub(super) fn thread_list(
    feed: Signal<Feed>,
    threads: super::threads::Threads,
    header: View,
    footer: View,
    empty: impl IntoElement,
    id: &'static str,
    row: impl Fn(ReadSignal<super::threads::Entry>) -> AnyElement + 'static,
) -> View {
    let entries = super::threads::entries(feed, threads);
    let row = stored(Box::new(row) as Box<dyn Fn(ReadSignal<super::threads::Entry>) -> AnyElement>);
    let list = virtual_list(
        move || entries.with(Vec::len),
        move |at| {
            let entry = computed(move || entries.with(|rows| rows.get(at.index()).cloned()));
            view().child(each(
                move || entry.get().into_iter().collect::<Vec<_>>(),
                super::threads::Entry::key,
                move |entry, _| row.with(|row| row(entry)),
            ))
        },
    )
    .test_id(id)
    .keys(move || {
        entries.with(|rows| {
            rows.iter()
                .map(super::threads::Entry::key)
                .collect::<Vec<_>>()
        })
    })
    .header(header)
    .footer(footer)
    .empty(empty)
    .flex_1();
    #[cfg(test)]
    let list = if let Some(probe) = use_context::<ViewabilityProbe>() {
        list.on_viewable_items_change(move |items: &nsh_core::element::ViewableItems| {
            probe.0.set(items.viewable.map(|(first, last)| VisibleRows {
                first,
                last,
                key: entries.with(|rows| {
                    rows.get(first)
                        .map_or_else(String::new, super::threads::Entry::key)
                }),
            }));
        })
    } else {
        list
    };
    view().flex_1().min_h_0().child(list)
}
