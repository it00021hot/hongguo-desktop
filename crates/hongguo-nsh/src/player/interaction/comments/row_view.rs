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
    pub logged_in: Memo<bool>,
    pub mine: Memo<bool>,
    pub reviews: bool,
    pub reply_stop: ElementRef,
    pub delete_stop: ElementRef,
}
pub(super) struct Events {
    pub like: Box<dyn Fn()>,
    pub reply: Box<dyn Fn()>,
    pub delete: Box<dyn Fn()>,
    pub image: Box<dyn Fn(ReadSignal<hongguo_api::emoticon::Emoticon>) -> AnyElement>,
    pub replies: Box<dyn Fn() -> AnyElement>,
}

pub(super) fn row(state: State, events: Events) -> View {
    let events = stored(events);
    let item = state.item;
    view().test_id(&format!("comment-{}", item.get_untracked().id())).flex_row().items_start().gap_3().py(move || px(if state.reviews {16.0} else {12.0}))
        .border_b_1().border_color(move || theme().border)
        .child(view().size(move || px(if state.reviews {36.0} else {32.0})).flex_shrink_0().rounded_full().overflow_hidden().bg(move || theme().muted)
            .child(when(move || !item.get().avatar().is_empty(), move || image(move || ImageSrc::from(crate::image_url::cdn_image_url(item.get().avatar()))).size_full().object_fit(nsh_core::ResizeMode::Cover)))
            .child(when(move || item.get().avatar().is_empty(), move || view().size_full().items_center().justify_center().child(text(move || item.get().name().chars().next().unwrap_or('?').to_string()).text_xs()))))
        .child(view().flex_1().min_w_0().gap_1()
            .child(view().flex_row().flex_wrap().items_baseline().gap_2()
                .child(text(move || if item.get().name().is_empty() {tr("player.comments.anon")} else {item.get().name().to_owned().into()}).text_sm().font_weight(FontWeight::MEDIUM))
                .child(when(move || state.reviews && matches!(item.get(), Item::Comment(ref comment) if !comment.score.is_empty()), move || {
                    ui::review_stars(move || match item.get() {Item::Comment(comment) => comment.score, Item::Reply(_) => String::new()})
                }))
                .child(text(move || date(&item.get(), state.reviews)).text_xs().text_color(move || theme().muted_foreground))
                .child(when(move || state.reviews, move || {
                    text(move || match item.get() {Item::Comment(comment) => comment.score_suffix_text, Item::Reply(_) => String::new()}).text_xs().text_color(move || theme().muted_foreground.opacity(0.7))
                })))
            .child(when(move || matches!(item.get(), Item::Reply(ref reply) if !reply.reply_to_name.is_empty()), move || {
                text(move || match item.get() {Item::Reply(reply) => tr("player.comments.replyToPrefixName").replace("{name}", &reply.reply_to_name), _ => String::new()}).text_xs().text_color(move || theme().muted_foreground)
            }))
            .child(emoji::render(move || item.get().text().to_owned(), ui::EmojiTextStyle::Body))
            .child(view().flex_row().flex_wrap().gap_1p5().child(each(move || item.get().images(), |image: &hongguo_api::emoticon::Emoticon| image.web_uri.clone(), move |image, _| events.with(|events| (events.image)(image)))))
            .child(view().flex_row().flex_wrap().items_center().gap_2()
                .child((if state.reviews {ui::review_like(move || item.get().like().0, move || item.get().like().1)} else {ui::button(move || {let (liked, count) = item.get().like(); format!("{} {count}", tr(if liked {"player.interact.liked"} else {"player.interact.like"}))}, ui::ButtonVariant::Quiet)})
                    .selected(move || item.get().like().0).disabled(move || !state.logged_in.get() || state.pending.get()).on_press(move || events.with(|events| (events.like)())))
                .child((if state.reviews {ui::review_link(move || tr("detail.reply"))} else {ui::button(move || match item.get() {Item::Comment(comment) => tr("player.comments.expandReplies").replace("{n}", &comment.reply_count.to_string()).into(), Item::Reply(_) => tr("player.comments.reply")}, ui::ButtonVariant::Quiet)})
                    .element_ref(&state.reply_stop).on_press(move || events.with(|events| (events.reply)())))
                .child(when(state.mine, move || (if state.reviews {ui::review_link(move || tr("common.delete"))} else {ui::button(move || tr("common.delete"), ui::ButtonVariant::Quiet)}).element_ref(&state.delete_stop).disabled(state.pending).on_press(move || events.with(|events| (events.delete)())))))
            .child(ui::status(state.error))
            .child(when(state.replies, move || events.with(|events| (events.replies)()))))
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
