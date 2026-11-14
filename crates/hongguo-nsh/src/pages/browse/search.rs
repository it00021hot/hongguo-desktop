use crate::{bridge, ui};
use hongguo_api::search::SuggestItem;
use nsh_core::prelude::*;
use std::{future::Future, rc::Rc, time::Duration};

const DEBOUNCE: Duration = Duration::from_millis(300);
const MAX_SUGGESTIONS: usize = 8;
const MIN_QUERY_CHARACTERS: usize = 2;

#[derive(Clone)]
struct Suggestions {
    query: String,
    items: Vec<SuggestItem>,
}

#[derive(Clone, Copy)]
pub(super) struct Search {
    draft: Signal<String>,
    context: Memo<Option<u64>>,
    suggestions: Resource<Suggestions, bridge::Failure>,
    opened: Signal<bool>,
    focused: Signal<bool>,
    pub composing: Signal<bool>,
    pub selected: Signal<usize>,
}

impl Search {
    pub fn new<F: Future<Output = Result<Vec<SuggestItem>, bridge::Failure>> + 'static>(
        draft: Signal<String>,
        context: impl Fn() -> Option<u64> + 'static,
        fetch: impl Fn(String) -> F + 'static,
    ) -> Self {
        let context = computed(context);
        let composing = signal(false);
        let fetch = Rc::new(fetch);
        let suggestions = resource(
            move || {
                let identity = context.get()?;
                let query = draft.get();
                (!composing.get() && query.trim().chars().count() >= MIN_QUERY_CHARACTERS)
                    .then_some((query, identity))
            },
            move |(query, _identity), _| {
                let fetch = fetch.clone();
                async move {
                    timer(DEBOUNCE).await;
                    let items = fetch(query.trim().to_owned()).await?;
                    Ok(Suggestions {
                        query,
                        items: items.into_iter().take(MAX_SUGGESTIONS).collect(),
                    })
                }
            },
        );
        Self {
            draft,
            context,
            suggestions,
            opened: signal(false),
            focused: signal(false),
            composing,
            selected: signal(0),
        }
    }

    pub fn visible(self) -> bool {
        self.context.get().is_some()
            && self.opened.get()
            && self.focused.get()
            && !self.composing.get()
            && !self.suggestions.loading()
            && self.suggestions.error().is_none()
            && self.suggestions.with(|response| {
                response.is_some_and(|response| {
                    response.query == self.draft.get() && !response.items.is_empty()
                })
            })
    }

    pub fn change(self, next: &str) {
        batch(|| {
            self.draft.set(next.to_owned());
            self.selected.set(0);
            self.opened.set(true);
        });
    }

    pub fn focus(self, focused: bool) {
        batch(|| {
            self.focused.set(focused);
            self.opened.set(focused);
        });
    }

    pub fn dismiss(self) {
        self.opened.set(false);
    }

    pub fn term(self) -> String {
        self.draft.get().trim().to_owned()
    }

    pub fn clear(self) {
        batch(|| {
            self.draft.set(String::new());
            self.opened.set(false);
            self.selected.set(0);
        });
    }

    pub fn step(self, delta: isize) {
        let count = self
            .suggestions
            .with(|response| response.map_or(0, |response| response.items.len()));
        if count > 0 {
            self.selected.update(|index| {
                *index = (*index as isize + delta).rem_euclid(count as isize) as usize
            });
        }
    }

    pub fn pick(self, index: usize) -> Option<SuggestItem> {
        if self.composing.get_untracked() || !self.visible() {
            return None;
        }
        let picked = self
            .suggestions
            .with(|response| response.and_then(|response| response.items.get(index).cloned()));
        self.dismiss();
        picked
    }

    pub fn popup(self) -> ui::SearchPopup {
        ui::SearchPopup {
            open: computed(move || self.visible()),
            composing: self.composing,
            selected: self.selected,
            options: computed(move || {
                self.suggestions.with(|response| {
                    response
                        .map(|response| {
                            response
                                .items
                                .iter()
                                .map(|item| ui::SearchSuggestion {
                                    word: item.word.clone().into(),
                                    cover: item.cover.clone(),
                                    description: item.abstract_text.clone().into(),
                                    rich: !item.series_id.is_empty(),
                                    parts: item
                                        .parts
                                        .iter()
                                        .map(|part| (part.text.clone().into(), part.hl))
                                        .collect(),
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
            }),
        }
    }
}

pub(super) fn field(
    search: Search,
    submit: impl Fn() + Copy + 'static,
    pick: impl Fn(SuggestItem) + Copy + 'static,
) -> View {
    let pick_index = move |index| {
        if let Some(item) = search.pick(index) {
            pick(item);
        }
    };
    let submit_input = move || {
        if search.composing.get_untracked() {
            return;
        }
        if search.visible() {
            pick_index(search.selected.get_untracked());
        } else {
            search.dismiss();
            submit();
        }
    };
    effect(move || {
        if !search.visible() {
            return;
        }
        for (key, delta) in [("up", -1), ("down", 1)] {
            on_key_with(
                key,
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    repeat: true,
                    ..Default::default()
                },
                move |_| search.step(delta),
            );
        }
        on_key_with(
            "enter",
            KeyOptions {
                when: nsh_core::keymap::When::Always,
                repeat: false,
                ..Default::default()
            },
            move |_| submit_input(),
        );
    });
    ui::search_suggestions(
        search.popup(),
        ui::search_field(move || crate::i18n::tr("search.placeholder"))
            .test_id("browse-query")
            .value(search.draft)
            .on_change(move |next: &str| search.change(next))
            .on_focus(move || search.focus(true))
            .on_blur(move || search.focus(false))
            .on_composition_change(move |event: &Composition| search.composing.set(event.composing))
            .on_submit(move |_: &str| submit_input()),
        pick_index,
        move || search.dismiss(),
    )
}
