use super::{
    feed::Context,
    model::{Entry, Query},
};
use crate::{bridge, events::SessionHandle, i18n::tr, ui};
use hongguo_service::library_service::{self, FolderMembership};
use nsh_base::theme;
use nsh_core::prelude::*;
use std::collections::HashSet;
use std::future::Future;

#[derive(Clone, Copy)]
pub(super) struct Membership {
    pub session: SessionHandle,
    pub context: Context,
    pub busy: Signal<bool>,
    pub feedback: Signal<SharedString>,
}

impl Membership {
    pub fn change(
        self,
        folder: i64,
        action: FolderMembership,
        entries: Vec<Entry>,
        done: impl FnOnce() + 'static,
    ) {
        if entries.is_empty() {
            return;
        }
        let state = self.session.with(|session| session.state.clone());
        self.run(
            library_service::update_collection_folder(state, folder, action, entries),
            done,
        );
    }

    pub fn run(
        self,
        request: impl Future<Output = hongguo_common::AppResult<()>> + Send + 'static,
        done: impl FnOnce() + 'static,
    ) {
        if self.busy.get_untracked() || !self.context.logged_in.get_untracked() {
            return;
        }
        let identity = self.context.identity.get_untracked();
        self.busy.set(true);
        self.feedback.set(SharedString::default());
        spawn(async move {
            let result = bridge::request(request).await;
            self.busy.set(false);
            if self.context.identity.get_untracked() != identity {
                return;
            }
            match result {
                Ok(()) => {
                    done();
                    self.session.notify_library();
                }
                Err(error) => self.feedback.set(error.text()),
            }
        });
    }
}

type Candidates = Resource<(u64, Vec<Entry>), bridge::Failure>;

#[derive(Clone, Copy)]
struct Picker {
    owner: Signal<Option<u64>>,
    search: Signal<String>,
    selected: Signal<HashSet<String>>,
    entries: Memo<Vec<Entry>>,
    visible: Memo<Vec<Entry>>,
    candidates: Candidates,
    busy: Signal<bool>,
}

pub(super) fn add(
    membership: Membership,
    folder: i64,
    existing: Memo<Vec<Entry>>,
    refresh: impl Fn() + Copy + 'static,
) -> View {
    let picker = Picker::new(membership, existing);
    let close = move || {
        if !picker.busy.get_untracked() {
            picker.owner.set(None);
        }
    };
    let submit = move || {
        if !picker.belongs_to(membership.context) {
            return;
        }
        membership.change(folder, FolderMembership::Add, picker.picked(), move || {
            picker.owner.set(None);
            refresh();
        });
    };
    view()
        .child(
            ui::button(
                move || tr("collections.folderAdd"),
                ui::ButtonVariant::Secondary,
            )
            .disabled(membership.busy)
            .on_press(move || {
                if membership.context.logged_in.get_untracked() {
                    picker.search.set(String::new());
                    picker.selected.update(HashSet::clear);
                    membership.feedback.set(SharedString::default());
                    picker
                        .owner
                        .set(Some(membership.context.identity.get_untracked()));
                }
            }),
        )
        .child(when(
            move || picker.owner.get().is_some(),
            move || dialog(picker, membership.feedback, close, submit),
        ))
}

impl Picker {
    fn new(membership: Membership, existing: Memo<Vec<Entry>>) -> Self {
        let owner = signal(None);
        let search = signal(String::new());
        let selected = signal(HashSet::<String>::new());
        let candidates = resource(
            move || {
                owner.get().filter(|owner| {
                    membership.context.logged_in.get()
                        && *owner == membership.context.identity.get()
                })
            },
            move |identity, _| {
                let state = membership.session.with(|session| session.state.clone());
                async move {
                    bridge::request(library_service::load(
                        state,
                        library_service::Kind::Collections,
                        Query::default(),
                    ))
                    .await
                    .map(|page| (identity, page.items))
                }
            },
        );
        effect(move || {
            if let Some(opened_for) = owner.get()
                && (!membership.context.logged_in.get()
                    || membership.context.identity.get() != opened_for)
            {
                owner.set(None);
                selected.update(HashSet::clear);
                search.set(String::new());
            }
        });
        let entries = computed(move || {
            let identity = membership.context.identity.get();
            if owner.get() != Some(identity) || !membership.context.logged_in.get() {
                return Vec::new();
            }
            existing.with(|existing| {
                let ids: HashSet<_> = existing
                    .iter()
                    .flat_map(|entry| {
                        std::iter::once(entry.series_id.as_str())
                            .chain(entry.collection_item_id.as_deref())
                    })
                    .collect();
                candidates.with(|result| {
                    result
                        .filter(|(current, _)| *current == identity)
                        .map_or_else(Vec::new, |(_, rows)| {
                            rows.iter()
                                .filter(|entry| {
                                    !entry.title.trim().is_empty()
                                        && !ids.contains(entry.series_id.as_str())
                                })
                                .cloned()
                                .collect()
                        })
                })
            })
        });
        let visible = computed(move || {
            let query = search.get().trim().to_lowercase();
            entries.with(|entries| {
                entries
                    .iter()
                    .filter(|entry| {
                        query.is_empty()
                            || entry.title.to_lowercase().contains(&query)
                            || entry.series_id.contains(&query)
                    })
                    .cloned()
                    .collect()
            })
        });
        Self {
            owner,
            search,
            selected,
            entries,
            visible,
            candidates,
            busy: membership.busy,
        }
    }

    fn belongs_to(self, context: Context) -> bool {
        context.logged_in.get_untracked()
            && self.owner.get_untracked() == Some(context.identity.get_untracked())
    }

    fn picked(self) -> Vec<Entry> {
        self.selected.with(|picked| {
            self.entries.with(|entries| {
                entries
                    .iter()
                    .filter(|entry| picked.contains(&entry.series_id))
                    .cloned()
                    .collect()
            })
        })
    }

    fn selected_count(self) -> usize {
        self.selected.with(|picked| {
            self.entries.with(|entries| {
                entries
                    .iter()
                    .filter(|entry| picked.contains(&entry.series_id))
                    .count()
            })
        })
    }
}

fn dialog(
    picker: Picker,
    feedback: Signal<SharedString>,
    close: impl Fn() + Copy + 'static,
    submit: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    super::folder_creation::form_dialog(
        "collections.folderAddTitle",
        460.0,
        picker.busy,
        close,
        view()
            .min_h_0()
            .child(
                view()
                    .gap_3()
                    .pb_3()
                    .flex_shrink_0()
                    .child(
                        text(move || tr("collections.folderAddTitle"))
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        super::presentation::library_search(picker.search, move || {
                            tr("collections.searchPlaceholder")
                        })
                        .w_full(),
                    ),
            )
            .child(
                view()
                    .min_h_0()
                    .flex_shrink(1.0)
                    .child(picker_results(picker)),
            )
            .child(ui::status(feedback))
            .child(picker_footer(picker, close, submit)),
    )
}

fn picker_results(picker: Picker) -> impl IntoElement {
    let candidates = picker.candidates;
    dynamic(move || {
        if let Some(error) = candidates.error() {
            return crate::features::error_state(error.text(), move || candidates.refetch())
                .into_element();
        }
        if picker.visible.with(Vec::is_empty) {
            if candidates.loading() && picker.entries.with(Vec::is_empty) {
                return ui::loading::indicator("common.loading")
                    .min_h(px(56.0))
                    .into_element();
            }
            let key = if picker.entries.with(Vec::is_empty) {
                "collections.folderAddEmpty"
            } else {
                "common.searchNoMatch"
            };
            return ui::status(move || tr(key)).into_element();
        }
        virtual_list(
            move || picker.visible.with(Vec::len),
            move |at| {
                picker_row(
                    picker,
                    computed(move || {
                        picker
                            .visible
                            .with(|entries| entries.get(at.index()).cloned())
                    }),
                )
            },
        )
        .h(move || {
            let count = picker.visible.with(Vec::len);
            let ideal = count as f32 * 56.0 + count.saturating_sub(1) as f32 * 4.0;
            px(ideal.min((window().height.0 * 0.52).clamp(56.0, 356.0)))
        })
        .row_gap(px(4.0))
        .estimated_item_size(px(56.0))
        .keys(move || {
            picker.visible.with(|entries| {
                entries
                    .iter()
                    .map(|entry| entry.series_id.clone())
                    .collect::<Vec<_>>()
            })
        })
        .into_element()
    })
}

fn picker_row(picker: Picker, entry: Memo<Option<Entry>>) -> impl IntoElement {
    let title = move || {
        entry.with(|entry| {
            entry
                .as_ref()
                .map_or_else(SharedString::default, |entry| entry.title.clone().into())
        })
    };
    let cover = computed(move || {
        entry.with(|entry| {
            entry
                .as_ref()
                .map(|entry| entry.cover.clone())
                .unwrap_or_default()
        })
    });
    let checked = computed(move || {
        entry.with(|row| {
            row.as_ref().is_some_and(|entry| {
                picker
                    .selected
                    .with(|picked| picked.contains(&entry.series_id))
            })
        })
    });
    super::presentation::library_check(title, checked, false)
        .disabled(move || picker.busy.get() || entry.with(Option::is_none))
        .hit_slop(0.0)
        .w_full()
        .h(px(56.0))
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .rounded(ui::radii::control)
        .hover(|style| style.bg(move || theme().muted))
        .checked_style(|style| style.bg(move || theme().accent))
        .child(
            view()
                .flex_1()
                .min_w_0()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    view()
                        .w(px(28.0))
                        .h(px(36.0))
                        .flex_shrink_0()
                        .rounded(ui::radii::control)
                        .overflow_hidden()
                        .bg(move || theme().muted)
                        .items_center()
                        .justify_center()
                        .child(when(
                            move || cover.with(String::is_empty),
                            move || {
                                ui::icon(ui::Icon::Collections)
                                    .size(px(17.0))
                                    .text_color(move || theme().muted_foreground)
                            },
                        ))
                        .child(when(
                            move || !cover.with(String::is_empty),
                            move || {
                                image(move || nsh_core::element::ImageSrc::from(cover.get()))
                                    .size_full()
                                    .object_fit(nsh_core::ResizeMode::Cover)
                                    .accessible(false)
                            },
                        )),
                )
                .child(
                    text(title)
                        .text_sm()
                        .line_height(px(18.0))
                        .number_of_lines(2)
                        .flex_1()
                        .min_w_0(),
                ),
        )
        .on_change(move |checked: &bool| {
            if let Some(entry) = entry.get_untracked() {
                picker.selected.update(|picked| {
                    if *checked {
                        picked.insert(entry.series_id);
                    } else {
                        picked.remove(&entry.series_id);
                    }
                });
            }
        })
}

fn picker_footer(picker: Picker, close: impl Fn() + 'static, submit: impl Fn() + 'static) -> View {
    view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .gap_2()
        .pt_3()
        .mt_3()
        .border_t_1()
        .border_color(move || theme().border)
        .child(
            text(move || {
                tr("batch.selectedCount").replace("{count}", &picker.selected_count().to_string())
            })
            .flex_1()
            .min_w_0()
            .text_sm()
            .text_color(move || theme().muted_foreground),
        )
        .child(
            ui::button(move || tr("common.cancel"), ui::ButtonVariant::Secondary)
                .disabled(picker.busy)
                .on_press(close),
        )
        .child(
            ui::button(
                move || tr("collections.folderAdd"),
                ui::ButtonVariant::Primary,
            )
            .disabled(move || picker.busy.get() || picker.selected_count() == 0)
            .on_press(submit),
        )
}
