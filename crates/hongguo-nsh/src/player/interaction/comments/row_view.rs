use super::model::Item;
use crate::{emoji, i18n::tr, ui};
use nsh_base::theme;
use nsh_core::{element::ImageSrc, prelude::*};

#[derive(Clone, Copy)]
pub(super) struct State {
    pub item: ReadSignal<Item>,
    pub pending: Signal<bool>,
    pub error: Signal<SharedString>,
    pub replies: Signal<bool>,
    pub editing: Signal<bool>,
    pub mine: Memo<bool>,
    pub reviews: bool,
    pub reply_stop: ElementRef,
    pub delete_stop: ElementRef,
}
pub(super) struct Events {
    pub like: Box<dyn Fn()>,
    pub reply: Box<dyn Fn()>,
    pub toggle_replies: Box<dyn Fn()>,
    pub delete: Box<dyn Fn()>,
    pub image: Box<dyn Fn(ReadSignal<hongguo_api::emoticon::Emoticon>) -> AnyElement>,
    pub replies: Box<dyn Fn() -> AnyElement>,
}

pub(super) fn row(state: State, events: Events) -> View {
    let events = stored(std::rc::Rc::new(events));
    let item = state.item;
    let reply = matches!(item.get_untracked(), Item::Reply(_));
    let review = state.reviews && !reply;
    let font = if reply {
        11.0
    } else if review {
        14.0
    } else {
        12.0
    };
    let meta = if reply {
        10.0
    } else if review {
        12.0
    } else {
        11.0
    };
    let paragraph = match (state.reviews, reply) {
        (true, true) => ui::EmojiTextStyle::ReviewReply,
        (true, false) => ui::EmojiTextStyle::Review,
        (false, true) => ui::EmojiTextStyle::Reply,
        (false, false) => ui::EmojiTextStyle::Comment,
    };
    let muted = move || {
        if state.reviews || !is_desktop() {
            theme().muted_foreground
        } else {
            theme().color("video-picker-muted").expect("picker muted")
        }
    };
    let like = move || {
        (if review {
            ui::review_like(move || item.get().like().0, move || item.get().like().1)
        } else {
            ui::comment_like(
                computed(move || item.get().like().0),
                computed(move || item.get().like().1),
                !state.reviews,
                reply,
            )
        })
        .disabled(state.pending)
        .on_press(move || (events.get().like)())
    };
    let metadata = view()
        .flex_row()
        .items_center()
        .gap_3()
        .mt(px(if review {
            6.0
        } else if reply {
            2.0
        } else {
            4.0
        }))
        .child(when(
            move || !review,
            move || {
                text(move || date(&item.get(), state.reviews))
                    .text_size(px(meta))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(muted)
            },
        ))
        .child(when(move || review, like))
        .child(
            ui::comment_link(move || tr("player.comments.reply"), !state.reviews, meta)
                .element_ref(&state.reply_stop)
                .on_press(move || (events.get().reply)()),
        )
        .child(when(state.mine, move || {
            ui::comment_link(move || tr("common.delete"), !state.reviews, meta)
                .element_ref(&state.delete_stop)
                .disabled(state.pending)
                .on_press(move || (events.get().delete)())
        }));
    let content = view()
        .flex_1()
        .min_w_0()
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    text(move || {
                        if item.get().name().is_empty() {
                            tr("player.comments.anon")
                        } else {
                            item.get().name().to_owned().into()
                        }
                    })
                    .text_size(px(font))
                    .font_weight(if review {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(move || if review { theme().foreground } else { muted() }),
                )
                .child(when(
                    move || review,
                    move || {
                        ui::review_stars(move || match item.get() {
                            Item::Comment(comment) => comment.score,
                            _ => String::new(),
                        })
                    },
                ))
                .child(when(
                    move || review,
                    move || {
                        text(move || date(&item.get(), true))
                            .text_xs()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(muted)
                    },
                ))
                .child(when(
                    move || review,
                    move || {
                        text(move || match item.get() {
                            Item::Comment(comment) => comment.score_suffix_text,
                            _ => String::new(),
                        })
                        .text_xs()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(move || theme().muted_foreground.opacity(0.7))
                    },
                )),
        )
        .child(
            view()
                .mt(px(if review { 4.0 } else { 2.0 }))
                .child(emoji::render(
                    move || match item.get() {
                        Item::Reply(reply) if !reply.reply_to_name.is_empty() => format!(
                            "{}{}",
                            tr("player.comments.replyToPrefixName")
                                .replace("{name}", &format!("{}\u{fffc}", reply.reply_to_name)),
                            reply.text
                        ),
                        item => item.text().to_owned(),
                    },
                    paragraph,
                )),
        )
        .child(when(
            move || !item.get().images().is_empty(),
            move || {
                view()
                    .mt(px(6.0))
                    .flex_row()
                    .flex_wrap()
                    .gap_1p5()
                    .child(each(
                        move || item.get().images(),
                        |image: &hongguo_api::emoticon::Emoticon| image.web_uri.clone(),
                        move |image, _| (events.get().image)(image),
                    ))
            },
        ))
        .child(metadata)
        .child(ui::status(state.error))
        .child(when(state.editing, move || (events.get().replies)()))
        .child(when(
            move || matches!(item.get(), Item::Comment(ref comment) if comment.reply_count > 0),
            move || {
                view().mt(px(if state.reviews { 8.0 } else { 6.0 })).child(
                    ui::comment_link(
                        move || {
                            if state.replies.get() {
                                tr("player.comments.collapseReplies")
                            } else {
                                tr("player.comments.expandReplies")
                                    .replace(
                                        "{n}",
                                        &match item.get() {
                                            Item::Comment(comment) => comment.reply_count,
                                            _ => 0,
                                        }
                                        .to_string(),
                                    )
                                    .into()
                            }
                        },
                        !state.reviews,
                        if state.reviews { 12.0 } else { 11.0 },
                    )
                    .on_press(move || (events.get().toggle_replies)()),
                )
            },
        ));
    let row = view()
        .test_id(&format!("comment-{}", item.get_untracked().id()))
        .flex_row()
        .items_start()
        .gap(px(if review {
            12.0
        } else if reply {
            8.0
        } else {
            10.0
        }))
        .py(px(if review {
            16.0
        } else if reply {
            4.0
        } else {
            8.0
        }))
        .child(
            view()
                .size(move || {
                    px(if review {
                        36.0
                    } else if state.reviews && reply && is_desktop() {
                        14.0
                    } else {
                        32.0
                    })
                })
                .flex_shrink_0()
                .rounded_full()
                .overflow_hidden()
                .bg(move || {
                    if state.reviews {
                        theme().muted
                    } else {
                        theme().color("video-picker-hover").expect("picker hover")
                    }
                })
                .child(when(
                    move || !item.get().avatar().is_empty(),
                    move || {
                        image(move || {
                            ImageSrc::from(crate::image_url::cdn_image_url(item.get().avatar()))
                        })
                        .size_full()
                        .object_fit(nsh_core::ResizeMode::Cover)
                    },
                ))
                .child(when(
                    move || item.get().avatar().is_empty(),
                    move || {
                        view().size_full().items_center().justify_center().child(
                            text(move || {
                                item.get().name().chars().next().unwrap_or('?').to_string()
                            })
                            .text_xs()
                            .font_weight(FontWeight::NORMAL),
                        )
                    },
                )),
        )
        .child(content)
        .child(when(
            move || !review,
            move || view().pt(px(if reply { 0.0 } else { 4.0 })).child(like()),
        ));
    if review {
        row.border_b(move || px(if state.replies.get() { 0.0 } else { 1.0 }))
            .pb(move || px(if state.replies.get() { 0.0 } else { 16.0 }))
            .border_color(move || theme().border)
    } else {
        row
    }
}

fn date(item: &Item, reviews: bool) -> String {
    let timestamp = match item {
        Item::Comment(item) => item.create_time,
        Item::Reply(item) => item.create_time,
    };
    if timestamp <= 0 {
        return String::new();
    }
    if !reviews || matches!(item, Item::Reply(_)) {
        let diff = hongguo_common::utils::time::now_ms().saturating_sub(timestamp);
        let (key, count) = if diff < 60_000 {
            ("player.comments.justNow", None)
        } else if diff < 3_600_000 {
            ("player.comments.minutesAgo", Some(diff / 60_000))
        } else if diff < 86_400_000 {
            ("player.comments.hoursAgo", Some(diff / 3_600_000))
        } else if diff < 172_800_000 {
            ("player.comments.yesterday", None)
        } else if diff < 2_592_000_000 {
            ("player.comments.daysAgo", Some(diff / 86_400_000))
        } else {
            ("", None)
        };
        if !key.is_empty() {
            return count.map_or_else(
                || tr(key).to_string(),
                |count| tr(key).replace("{n}", &count.to_string()),
            );
        }
        return chrono::DateTime::from_timestamp_millis(timestamp)
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%-m/%-d")
                    .to_string()
            })
            .unwrap_or_default();
    }
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format(if crate::i18n::is_chinese() {
                    "%Y/%-m/%-d"
                } else {
                    "%-m/%-d/%Y"
                })
                .to_string()
        })
        .unwrap_or_default()
}
