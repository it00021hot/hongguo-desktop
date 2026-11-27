use super::{feed::Context, folder_membership::Membership};
use crate::{events::SessionHandle, i18n::tr, ui};
use hongguo_service::library_service;
use nsh_base::theme;
use nsh_core::prelude::*;

const NAME_LIMIT: u32 = 15;

pub(super) enum NameAction {
    Create,
    Edit {
        id: i64,
        name: String,
        published: bool,
    },
}

impl NameAction {
    fn title(&self) -> &'static str {
        match self {
            Self::Create => "collections.createFolder",
            Self::Edit { .. } => "collections.folderSettings",
        }
    }
}

pub(super) fn action(
    session: SessionHandle,
    context: Context,
    refresh: impl Fn() + Copy + 'static,
) -> View {
    let open = signal(false);
    let membership = Membership {
        session,
        context,
        busy: signal(false),
        feedback: signal(SharedString::default()),
    };
    view()
        .child(
            ui::button(
                move || tr("collections.createFolder"),
                ui::ButtonVariant::Secondary,
            )
            .on_press(move || {
                membership.feedback.set(SharedString::default());
                open.set(true);
            }),
        )
        .child(dialog(
            membership,
            open,
            signal(false),
            NameAction::Create,
            move |_, _| refresh(),
            view,
        ))
}

pub(super) fn dialog(
    membership: Membership,
    open: Signal<bool>,
    suspended: Signal<bool>,
    action: NameAction,
    after: impl Fn(String, bool) + Copy + 'static,
    extra: impl Fn() -> View + Copy + 'static,
) -> View {
    let action = stored(action);
    let key = action.with(|action| action.title());
    let editing = action.with(|action| matches!(action, NameAction::Edit { .. }));
    let close = move || {
        if !membership.busy.get_untracked() {
            open.set(false);
        }
    };
    view().child(when(open, move || {
        let initial = action.with(|action| match action {
            NameAction::Create => (String::new(), true),
            NameAction::Edit {
                name, published, ..
            } => (name.clone(), *published),
        });
        let name = signal(initial.0.clone());
        let publish = signal(initial.1);
        let initial = stored(initial);
        let valid = computed(move || {
            name.with(|name| {
                let name = name.trim();
                !name.is_empty()
                    && name.encode_utf16().count() <= NAME_LIMIT as usize
                    && initial
                        .with(|initial| initial.0.as_str() != name || initial.1 != publish.get())
            })
        });
        let submit = move || {
            if !valid.get_untracked() {
                return;
            }
            let trimmed = name.get_untracked().trim().to_owned();
            let saved_name = trimmed.clone();
            let edit = action.with(|action| match action {
                NameAction::Create => None,
                NameAction::Edit { id, .. } => Some(*id),
            });
            let state = membership.session.with(|session| session.state.clone());
            let publish = publish.get_untracked();
            membership.run(
                async move {
                    if let Some(id) = edit {
                        library_service::update_collection_folder_settings(
                            state, id, trimmed, publish,
                        )
                        .await
                    } else {
                        library_service::create_collection_folder(state, trimmed, publish).await
                    }
                },
                move || {
                    open.set(false);
                    after(saved_name, publish);
                },
            );
        };
        when(
            move || !suspended.get(),
            move || {
                form_dialog(
                    key,
                    if editing { 512.0 } else { 440.0 },
                    membership.busy,
                    close,
                    view()
                        .gap_4()
                        .child(
                            text(move || tr(key))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_lg(),
                        )
                        .child(
                            view()
                                .gap_2()
                                .child(
                                    text(move || tr("collections.folderNameLabel"))
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM),
                                )
                                .child(
                                    ui::field(move || tr("collections.folderNamePlaceholder"))
                                        .auto_focus(true)
                                        .max_length(NAME_LIMIT)
                                        .value(name)
                                        .disabled(membership.busy)
                                        .on_change(move |input: &str| name.set(input.to_owned()))
                                        .on_submit(move |_: &str| submit()),
                                ),
                        )
                        .child(
                            view().flex_row().justify_end().child(
                                text(move || {
                                    format!(
                                        "{}/{}",
                                        name.with(|name| name.encode_utf16().count()),
                                        NAME_LIMIT
                                    )
                                })
                                .text_xs()
                                .text_color(move || theme().muted_foreground),
                            ),
                        )
                        .child(visibility(publish, membership.busy))
                        .child(extra())
                        .child(ui::status(membership.feedback))
                        .child(
                            view()
                                .flex_row()
                                .justify_end()
                                .gap_2()
                                .child(
                                    ui::button(
                                        move || tr("common.cancel"),
                                        ui::ButtonVariant::Secondary,
                                    )
                                    .disabled(membership.busy)
                                    .on_press(close),
                                )
                                .child(
                                    ui::button(
                                        move || {
                                            tr(if editing {
                                                "collections.folderSave"
                                            } else {
                                                "common.confirm"
                                            })
                                        },
                                        ui::ButtonVariant::Primary,
                                    )
                                    .disabled(move || membership.busy.get() || !valid.get())
                                    .on_press(submit),
                                ),
                        ),
                )
            },
        )
    }))
}

/// Centered folder form on NSH's native modal layer, with the same motion,
/// theme and dismissal conventions as the account dialog.
pub(super) fn form_dialog(
    title: &'static str,
    width: f32,
    busy: Signal<bool>,
    close: impl Fn() + 'static,
    content: impl IntoElement,
) -> impl IntoElement {
    nsh_base::dialog()
        .label(move || tr(title))
        .animation(move || {
            if reduce_motion() {
                ModalAnimation::None
            } else {
                ModalAnimation::Fade
            }
        })
        .close_on_escape(move || !busy.get())
        .close_on_backdrop_press(false)
        .on_cancel(close)
        .child(
            view()
                .flex_1()
                .pb_keyboard()
                .items_center()
                .justify_center()
                .p_4()
                .bg(move || {
                    theme()
                        .color("dialog-backdrop")
                        .expect("dialog backdrop token")
                })
                .child(
                    view()
                        .w(px(width))
                        .max_w(percentage(1.0))
                        .max_h(percentage(0.9))
                        .min_h_0()
                        .p_5()
                        .rounded(move || theme().radius)
                        .border_1()
                        .border_color(move || theme().border)
                        .bg(move || theme().background)
                        .text_color(move || theme().foreground)
                        .shadow_lg()
                        .child(content),
                ),
        )
}

fn visibility(publish: Signal<bool>, busy: Signal<bool>) -> View {
    view()
        .gap_2()
        .child(
            text(move || tr("collections.folderVisibility"))
                .text_sm()
                .font_weight(FontWeight::MEDIUM),
        )
        .child(
            view().flex_row().flex_wrap().gap_2().children(
                [
                    (
                        false,
                        "collections.folderPrivate",
                        "collections.folderPrivateHint",
                    ),
                    (
                        true,
                        "collections.folderPublic",
                        "collections.folderPublicHint",
                    ),
                ]
                .into_iter()
                .map(move |(value, label, hint)| {
                    ui::controls::surface(ui::ButtonVariant::Secondary)
                        .accessibility_label(move || Some(tr(label)))
                        .selected(move || publish.get() == value)
                        .disabled(busy)
                        .flex_1()
                        .min_w(px(150.0))
                        .items_stretch()
                        .p_3()
                        .on_press(move || publish.set(value))
                        .child(
                            view()
                                .gap_1()
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            text(move || tr(label))
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD),
                                        )
                                        .child(when(
                                            move || publish.get() == value,
                                            || ui::icon(ui::Icon::Check).size(px(16.0)),
                                        )),
                                )
                                .child(
                                    text(move || tr(hint))
                                        .text_xs()
                                        .text_color(move || theme().muted_foreground),
                                ),
                        )
                }),
            ),
        )
}
