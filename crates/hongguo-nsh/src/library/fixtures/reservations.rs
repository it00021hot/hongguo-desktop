use super::*;

pub(super) fn build(nav: Navigation, entries: Vec<Entry>) -> View {
    let entries = signal(entries);
    let logged_in = signal(true);
    on_key("mod+shift+a", move |_| {
        logged_in.update(|value| *value = !*value)
    });
    let memory = crate::view_memory::scope("reservations");
    let online = memory.signal("online", true);
    let search = memory.signal("search", String::new());
    let query = signal(Query {
        online: online.get_untracked(),
        ..Default::default()
    });
    let busy = signal(false);
    let managing = signal(false);
    let selected = signal(Vec::<String>::new());
    let visible = computed(move || {
        if !logged_in.get() {
            return Vec::new();
        }
        let query = search.get().trim().to_lowercase();
        entries
            .get()
            .into_iter()
            .filter(|entry| {
                entry.can_play == online.get()
                    && (query.is_empty()
                        || entry.title.to_lowercase().contains(&query)
                        || entry.series_id.contains(&query))
            })
            .collect::<Vec<_>>()
    });
    let rows = Rows {
        kind: Kind::Reservations,
        nav,
        items: visible,
        busy,
        managing,
        selected,
        has_more: computed(|| false),
        scroll_scope: computed(move || (u64::from(logged_in.get()), online.get(), search.get(), 0)),
    };
    let remove = move |removed: Vec<Entry>| {
        entries.update(|entries| {
            entries.retain(|entry| !removed.iter().any(|removed| removed.key == entry.key))
        });
        selected
            .update(|keys| keys.retain(|key| !removed.iter().any(|removed| &removed.key == key)));
    };
    let feedback = signal(SharedString::default());
    let toolbar = toolbar::build(Toolbar {
        kind: Kind::Reservations,
        query,
        search,
        online,
        category: signal(0),
        busy,
        managing,
        selected,
        logged_in: computed(move || logged_in.get()),
        counts: computed(move || {
            logged_in.get().then(|| {
                let online = entries
                    .with(|entries| entries.iter().filter(|item| item.can_play).count() as i64);
                (online, entries.with(Vec::len) as i64 - online)
            })
        }),
    });
    super::super::presentation::frame(
        Kind::Reservations,
        toolbar,
        computed(move || logged_in.get()),
        nav,
        move || {
            view()
                .flex_1()
                .min_h_0()
                .gap_4()
                .child(rows::build(
                    rows,
                    remove,
                    move |entry| nav.push(Route::Detail(entry.series_id)),
                    || {},
                ))
                .child(toolbar::batch(rows, remove, move |error| {
                    feedback.set(error)
                }))
                .child(ui::status(feedback))
        },
    )
}
