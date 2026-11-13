use super::{
    SYSTEM, attachment,
    remote_model::{Feed, Tab},
};
use crate::{i18n::tr, ui};
use hongguo_api::emoticon::Emoticon;
use nsh_core::prelude::*;

#[derive(Clone)]
pub(super) enum Intent {
    Tab(Tab),
    Search(String),
    Insert(&'static str),
    Pick(Emoticon, Tab),
    Collect(Emoticon, bool),
    Upload,
    More,
    Retry,
    Login,
}

#[derive(Clone, Copy)]
pub(super) struct State {
    pub feed: Signal<Feed>,
    pub busy: Memo<bool>,
    pub notice: Signal<SharedString>,
    pub failed: Memo<bool>,
}

pub(super) fn panel(
    state: State,
    images: bool,
    action: impl Fn(Intent) + Copy + 'static,
) -> impl IntoElement {
    let tab = computed(move || state.feed.with(|feed| feed.query.tab));
    ui::emoji_panel()
        .child(
            view().flex_row().gap_1().children(
                [
                    (
                        Tab::Search,
                        "player.emoticon.search",
                        Some(ui::Icon::Search),
                    ),
                    (Tab::System, "player.emoticon.system", Some(ui::Icon::Smile)),
                    (
                        Tab::Own,
                        "player.emoticon.custom",
                        Some(ui::Icon::Collections),
                    ),
                    (Tab::ThirdParty, "player.emoticon.thirdParty", None),
                ]
                .into_iter()
                .filter(move |(kind, _, _)| images || *kind == Tab::System)
                .map(move |(kind, label, icon)| {
                    ui::emoji_tab(move || tr(label), icon)
                        .selected(move || tab.get() == kind)
                        .on_press(move || action(Intent::Tab(kind)))
                }),
            ),
        )
        .child(when(
            move || tab.get() == Tab::Search,
            move || {
                ui::emoji_search()
                    .value(move || state.feed.with(|feed| feed.query.keyword.clone()))
                    .on_change(move |value: &str| action(Intent::Search(value.into())))
            },
        ))
        .child(when(
            move || tab.get() == Tab::System,
            move || {
                scroll().max_h_52().child(dynamic(move || {
                    let columns = ui::emoji_metrics(true).0;
                    view()
                        .gap_0p5()
                        .children(SYSTEM.chunks(columns).map(move |row| {
                            view()
                                .flex_row()
                                .gap_0p5()
                                .children(row.iter().map(move |item| {
                                    ui::emoji_system_cell(item.code, item.uri)
                                        .on_press(move || action(Intent::Insert(item.code)))
                                }))
                        }))
                }))
            },
        ))
        .child(when(
            move || tab.get() != Tab::System,
            move || {
                let small = computed(move || tab.get() == Tab::Own);
                let columns = computed(move || ui::emoji_metrics(small.get()).0);
                let count = computed(move || {
                    state.feed.with(|feed| feed.items.len()) + usize::from(small.get())
                });
                view()
                    .gap_2()
                    .child(when(
                        move || {
                            tab.get() == Tab::Own
                                && state.feed.with(|feed| feed.query.account.is_none())
                        },
                        move || {
                            ui::button(
                                move || tr("settings.loginAction"),
                                ui::ButtonVariant::Secondary,
                            )
                            .on_press(move || action(Intent::Login))
                        },
                    ))
                    .child(
                        virtual_list(
                            move || count.get().div_ceil(columns.get()),
                            move |at| {
                                view()
                                    .flex_row()
                                    .gap(move || px(ui::emoji_metrics(small.get()).2))
                                    .children((0..8).map(move |column| {
                                        let index =
                                            computed(move || at.index() * columns.get() + column);
                                        let upload =
                                            computed(move || small.get() && index.get() == 0);
                                        let item = computed(move || {
                                            if column >= columns.get() || upload.get() {
                                                return None;
                                            }
                                            let offset = index
                                                .get()
                                                .checked_sub(usize::from(small.get()))?;
                                            state.feed.with(|feed| feed.items.get(offset).cloned())
                                        });
                                        dynamic(move || {
                                            if column >= columns.get() {
                                                return view().into_element();
                                            }
                                            if upload.get() {
                                                return ui::emoji_tab(
                                                    move || tr("player.emoticon.uploadCustom"),
                                                    Some(ui::Icon::Upload),
                                                )
                                                .size(move || px(ui::emoji_metrics(true).1))
                                                .disabled(state.busy)
                                                .on_press(move || action(Intent::Upload))
                                                .into_element();
                                            }
                                            let Some(current) = item.get() else {
                                                return view().into_element();
                                            };
                                            let label: SharedString = current
                                                .image_name
                                                .clone()
                                                .filter(|name| !name.is_empty())
                                                .map(Into::into)
                                                .unwrap_or_else(|| tr("player.emoticon.image"));
                                            ui::emoji_context(
                                                ui::emoji_image_cell(
                                                    move || {
                                                        item.get()
                                                            .map_or_else(String::new, |item| {
                                                                attachment::preview(&item)
                                                            })
                                                    },
                                                    label,
                                                    small.get_untracked(),
                                                )
                                                .disabled(state.busy)
                                                .on_press(move || {
                                                    if let Some(item) = item.get_untracked() {
                                                        action(Intent::Pick(
                                                            item,
                                                            tab.get_untracked(),
                                                        ));
                                                    }
                                                }),
                                                move || small.get(),
                                                move || {
                                                    if let Some(item) = item.get_untracked() {
                                                        action(Intent::Collect(
                                                            item,
                                                            small.get_untracked(),
                                                        ));
                                                    }
                                                },
                                            )
                                            .into_element()
                                        })
                                    }))
                            },
                        )
                        .h_52()
                        .gap(move || px(ui::emoji_metrics(small.get()).2)),
                    )
                    .child(when(
                        move || state.busy.get(),
                        move || ui::emoji_hint(move || tr("common.loading")),
                    ))
                    .child(when(
                        move || state.failed.get(),
                        move || {
                            ui::button(move || tr("common.retry"), ui::ButtonVariant::Quiet)
                                .on_press(move || action(Intent::Retry))
                        },
                    ))
                    .child(when(
                        move || {
                            !state.busy.get()
                                && !state.failed.get()
                                && count.get() == 0
                                && state.feed.with(|feed| feed.accepts_load())
                        },
                        move || ui::emoji_hint(move || tr("player.emoticon.empty")),
                    ))
                    .child(when(
                        move || state.feed.with(|feed| feed.has_more),
                        move || {
                            ui::button(
                                move || tr("player.emoticon.loadMore"),
                                ui::ButtonVariant::Quiet,
                            )
                            .disabled(state.busy)
                            .on_press(move || action(Intent::More))
                        },
                    ))
            },
        ))
        .child(ui::status(state.notice))
}
