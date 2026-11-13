use super::{ButtonVariant, Icon, controls::surface};
use crate::i18n::tr;
use hongguo_common::model::{FollowNotice, NOTICE_KIND_ONLINE};
use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone)]
pub(crate) enum NoticeAction {
    Check,
    Clear,
    Open(FollowNotice),
    Popup(bool),
}

#[derive(Clone, Copy)]
pub(crate) struct FollowNoticesState {
    pub(crate) items: Memo<Vec<FollowNotice>>,
    pub(crate) busy: Memo<bool>,
    pub(crate) checking: Memo<bool>,
    pub(crate) popup: Signal<bool>,
}

pub(crate) fn follow_notices(
    state: FollowNoticesState,
    action: impl Fn(NoticeAction) + Copy + 'static,
) -> View {
    view()
        .test_id("follow-notices-panel")
        .flex_1()
        .min_h(px(0.0))
        .gap_3()
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    super::library_check(move || tr("follow.popupOn"), state.popup, || false)
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .min_h(move || px(if is_desktop() { 20.0 } else { 48.0 }))
                        .child(text(move || tr("follow.popupOn")).text_sm())
                        .disabled(state.busy)
                        .on_change(move |value: &bool| action(NoticeAction::Popup(*value))),
                )
                .child(
                    super::library_action(
                        move || {
                            tr(if state.checking.get() {
                                "follow.checking"
                            } else {
                                "follow.checkUpdate"
                            })
                        },
                        Some(Icon::Retry),
                    )
                    .disabled(state.busy)
                    .on_press(move || action(NoticeAction::Check)),
                )
                .child(when(
                    move || !state.items.with(Vec::is_empty),
                    move || {
                        surface(ButtonVariant::Quiet)
                            .accessibility_label(move || tr("follow.clearAll"))
                            .flex_row()
                            .gap_1p5()
                            .px_2p5()
                            .py_1()
                            .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
                            .child(super::icon(Icon::CheckCheck).size_4())
                            .child(
                                text(move || tr("follow.clearAll"))
                                    .text_sm()
                                    .line_height(px(20.0))
                                    .font_weight(FontWeight::MEDIUM),
                            )
                            .disabled(state.busy)
                            .on_press(move || action(NoticeAction::Clear))
                    },
                )),
        )
        .child(when(
            move || state.items.with(Vec::is_empty),
            move || {
                view()
                    .py_16()
                    .items_center()
                    .gap_2()
                    .child(super::icon(Icon::Reservations).size_8().opacity(0.4))
                    .child(
                        text(move || tr("follow.noticesEmpty"))
                            .text_sm()
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(
                        text(move || tr("follow.noticesEmptyHint"))
                            .text_xs()
                            .text_color(move || theme().muted_foreground),
                    )
            },
        ))
        .child(
            virtual_list(
                move || state.items.with(Vec::len),
                move |position| {
                    let item = computed(move || {
                        state
                            .items
                            .with(|items| items.get(position.index()).cloned())
                    });
                    view()
                        .w_full()
                        .test_id("follow-notice")
                        .child(
                            surface(ButtonVariant::Quiet)
                                .accessibility_label(move || {
                                    item.get().map(|item| notice_label(&item))
                                })
                                .w_full()
                                .min_h(move || px(if is_desktop() { 62.0 } else { 72.0 }))
                                .p_3()
                                .mb_2()
                                .rounded(super::radii::card)
                                .border_1()
                                .border_color(move || {
                                    if item.get().is_some_and(|item| !item.read) {
                                        theme().primary.opacity(0.4)
                                    } else {
                                        theme().border
                                    }
                                })
                                .bg(move || theme().card)
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .disabled(state.busy)
                                .on_press(move || {
                                    if let Some(item) = item.get_untracked() {
                                        action(NoticeAction::Open(item));
                                    }
                                })
                                .child(notice_badge(item))
                                .child(
                                    view()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .child(
                                            text(move || {
                                                item.get().map_or(SharedString::default(), |item| {
                                                    item.title.into()
                                                })
                                            })
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .number_of_lines(1),
                                        )
                                        .child(
                                            text(move || {
                                                item.get().map_or(SharedString::default(), |item| {
                                                    item.detail.into()
                                                })
                                            })
                                            .text_xs()
                                            .text_color(move || theme().muted_foreground)
                                            .number_of_lines(1),
                                        ),
                                )
                                .child(
                                    text(move || {
                                        item.get()
                                            .map_or(String::new(), |item| notice_time(item.at))
                                    })
                                    .text_xs()
                                    .flex_shrink_0()
                                    .text_color(move || theme().muted_foreground),
                                )
                                .child(when(
                                    move || item.get().is_some_and(|item| !item.read),
                                    move || {
                                        view()
                                            .size_2()
                                            .rounded_full()
                                            .flex_shrink_0()
                                            .bg(move || theme().destructive)
                                    },
                                )),
                        )
                        .into_element()
                },
            )
            .keys(move || {
                state
                    .items
                    .get()
                    .into_iter()
                    .map(|item| item.id.to_string())
                    .collect::<Vec<_>>()
            })
            .estimated_item_size(px(80.0))
            .flex_1()
            .min_h(px(0.0)),
        )
}

pub(crate) fn follow_notice_feedback(message: Signal<SharedString>) -> View {
    const FEEDBACK_LIFETIME: std::time::Duration = std::time::Duration::from_millis(2500);
    effect(move || {
        if !message.get().is_empty() {
            let deadline = set_timeout(FEEDBACK_LIFETIME, move || {
                message.set(SharedString::default())
            });
            cleanup(move || deadline.cancel());
        }
    });
    view()
        .absolute()
        .right(move || px(if is_desktop() { 24.0 } else { 16.0 }))
        .bottom(move || px(if is_desktop() { 24.0 } else { 16.0 }))
        .w(px(356.0))
        .max_w(move || px(window().width.0 - 32.0))
        .p_4()
        .border_1()
        .border_color(move || theme().border)
        .rounded(super::radii::control)
        .bg(move || theme().popover)
        .text_color(move || theme().popover_foreground)
        .shadow_lg()
        .z_index(50)
        .role(AccessibilityRole::Alert)
        .accessible(true)
        .style_when(move || message.get().is_empty(), |style| style.hidden())
        .child(
            text(message)
                .text_size(px(13.0))
                .line_height(px(19.5))
                .font_weight(FontWeight::MEDIUM),
        )
}

pub(crate) fn follow_login_required(on_login: impl Fn() + 'static) -> View {
    super::personal_login_required("follow.loginRequired", Icon::Login, on_login)
}

fn notice_label(item: &FollowNotice) -> SharedString {
    let kind = tr(if item.kind == NOTICE_KIND_ONLINE {
        "follow.kindOnline"
    } else {
        "follow.kindFollow"
    });
    let state = tr(if item.read {
        "follow.read"
    } else {
        "follow.unread"
    });
    format!("{} · {} · {kind} · {state}", item.title, item.detail).into()
}

fn notice_badge(item: Memo<Option<FollowNotice>>) -> View {
    let online = move || {
        item.get()
            .is_some_and(|item| item.kind == NOTICE_KIND_ONLINE)
    };
    view()
        .flex_shrink_0()
        .rounded(px(4.0))
        .px_1p5()
        .py_0p5()
        .bg(move || {
            if online() {
                theme().primary.opacity(0.1)
            } else {
                theme().muted
            }
        })
        .text_color(move || {
            if online() {
                theme().primary
            } else {
                theme().muted_foreground
            }
        })
        .child(
            text(move || {
                tr(if online() {
                    "follow.kindOnline"
                } else {
                    "follow.kindFollow"
                })
            })
            .text_size(px(10.0))
            .font_weight(FontWeight::MEDIUM),
        )
}

fn notice_time(seconds: i64) -> String {
    use chrono::{DateTime, Local};
    DateTime::from_timestamp(seconds, 0).map_or(String::new(), |date| {
        date.with_timezone(&Local)
            .format(if crate::i18n::is_chinese() {
                "%-m月%-d日 %H:%M"
            } else {
                "%b %-d, %I:%M %p"
            })
            .to_string()
    })
}
