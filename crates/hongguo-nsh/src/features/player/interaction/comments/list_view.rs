use super::model::Feed;
use nsh_core::prelude::*;

pub(super) fn thread_list(
    feed: Signal<Feed>,
    threads: super::threads::Threads,
    header: View,
    footer: View,
    empty: impl IntoElement,
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
    view().flex_1().min_h_0().child(list)
}
