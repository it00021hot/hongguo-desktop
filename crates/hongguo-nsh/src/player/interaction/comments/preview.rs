//! No database, SessionHandle or API: the production form/list/row with local events.
use super::{
    list_view,
    model::{Feed, Item},
    review_form,
    review_model::ReviewSummary,
    row_view,
};
use crate::{emoji, ui};
use hongguo_api::danmaku::{CommentItem, CommentTagStat, ReplyItem};
use nsh_core::prelude::*;

pub(crate) fn reviews_preview_with_header(header: View) -> View {
    let editor = emoji::Editor::new(signal(String::new()), 500);
    let stars = signal(0_u8);
    let output = signal(SharedString::default());
    let next_id = signal(100_u32);
    let feed = signal(Feed::default());
    feed.update(|feed| {
        feed.items = (0..80)
            .map(|index| {
                Item::Comment(CommentItem {
                    comment_id: format!("review-preview-{index}"),
                    user_id: format!("reader-{index}"),
                    user_name: format!("第{}位观众", index + 1),
                    text: if index == 0 {
                        "剧情很精彩[微笑]，人物也很生动。".into()
                    } else {
                        format!("这是第{}条离线剧评，推荐大家看看。", index + 1)
                    },
                    score: if index == 0 { "7".into() } else { "8".into() },
                    score_suffix_text: "观看1小时后点评".into(),
                    create_time: 1_794_355_200_000,
                    reply_count: i64::from(index == 0),
                    digg_count: 12,
                    ..Default::default()
                })
            })
            .collect();
        feed.summary = Some(ReviewSummary {
            score: "8.3".into(),
            count: 1001,
            tags: vec![CommentTagStat {
                tag_name: "人物生动".into(),
                count: 26,
            }],
        });
        feed.total = 80;
    });
    let form = review_form::form(
        editor,
        stars,
        signal(false),
        computed(|| true),
        review_form::Events {
            login: Box::new(|| {}),
            send: Box::new(move || {
                let Some(score) = super::review_model::review_score(stars.get_untracked()) else {
                    return;
                };
                let text = editor.text.get_untracked().trim().to_owned();
                if text.is_empty() {
                    return;
                }
                let id = next_id.get_untracked();
                next_id.set(id + 1);
                feed.update(|feed| {
                    feed.accept_sent(Item::Comment(CommentItem {
                        comment_id: format!("sent-{id}"),
                        user_id: "offline".into(),
                        user_name: "离线测试账号".into(),
                        text,
                        score: score.to_string(),
                        score_suffix_text: "刚刚发表".into(),
                        ..Default::default()
                    }))
                });
                editor.text.set(String::new());
                output.set(format!("离线发表成功：{score}分").into());
            }),
        },
    );
    let heading = view()
        .child(header)
        .child(
            ui::detail_section()
                .gap_2()
                .child(form)
                .child(ui::review_summary(
                    move || {
                        feed.get()
                            .summary
                            .map_or_else(String::new, |summary| summary.score)
                    },
                    move || feed.get().summary.map_or(0, |summary| summary.count),
                    move || {
                        feed.get()
                            .summary
                            .map_or_else(Vec::new, |summary| summary.tags)
                    },
                )),
        );
    list_view::list(
        feed,
        heading,
        ui::detail_section()
            .pb(move || {
                px(if window().width.0 < 768.0 {
                    144.0
                } else {
                    24.0
                })
            })
            .child(ui::status(output)),
        view(),
        "reviews-list",
        move |item| {
            ui::detail_section()
                .child(fixture_row(item, feed))
                .into_element()
        },
    )
}

fn fixture_row(item: ReadSignal<Item>, feed: Signal<Feed>) -> View {
    let replies = signal(item.get_untracked().id() == "review-preview-0");
    row_view::row(
        row_view::State {
            item,
            pending: signal(false),
            error: signal(SharedString::default()),
            replies,
            logged_in: computed(|| true),
            mine: computed(move || item.get().user_id() == "offline"),
            reviews: true,
            reply_stop: element_ref(),
            delete_stop: element_ref(),
        },
        row_view::Events {
            like: Box::new(move || {
                feed.update(|feed| {
                    feed.toggle_like(item.get_untracked().id());
                })
            }),
            reply: Box::new(move || replies.update(|open| *open = !*open)),
            delete: Box::new(move || feed.update(|feed| feed.remove(item.get_untracked().id()))),
            image: Box::new(move |item| {
                ui::emoji_comment_image(move || {
                    nsh_core::element::ImageSrc::from(item.get().web_uri)
                })
                .into_element()
            }),
            replies: Box::new(move || {
                reply_preview(item.get_untracked().id().to_owned(), feed).into_element()
            }),
        },
    )
}

fn reply_preview(parent: String, root: Signal<Feed>) -> View {
    let parent = stored(parent);
    let feed = signal(Feed::default());
    feed.update(|feed| {
        feed.accept_sent(Item::Reply(ReplyItem {
            reply_id: format!("{}-reply", parent.get()),
            user_name: "回复的观众".into(),
            user_id: "reply-reader".into(),
            reply_to_name: "第1位观众".into(),
            text: "确实很好看[偷笑]".into(),
            digg_count: 2,
            ..Default::default()
        }))
    });
    let editor = emoji::Editor::new(signal(String::new()), 200);
    let focus = super::focus::Focus::new();
    let next = element_ref();
    view()
        .element_ref(&focus.list)
        .role(nsh_core::AccessibilityRole::Group)
        .label(move || crate::i18n::tr("player.comments.title"))
        .focusable(true)
        .gap_2()
        .pl_3()
        .child(each(
            move || feed.get().items,
            |item: &Item| item.id().to_owned(),
            move |item, _| {
                let mine = computed(move || item.get().user_id() == "offline");
                let pending = signal(false);
                let (reply_stop, delete_stop) =
                    focus.register(item.get_untracked().id().to_owned(), mine, pending);
                row_view::row(
                    row_view::State {
                        item,
                        pending,
                        error: signal(SharedString::default()),
                        replies: signal(false),
                        logged_in: computed(|| true),
                        mine,
                        reviews: true,
                        reply_stop,
                        delete_stop,
                    },
                    row_view::Events {
                        like: Box::new(move || {
                            feed.update(|feed| {
                                feed.toggle_like(item.get_untracked().id());
                            })
                        }),
                        reply: Box::new(move || editor.reference.focus()),
                        delete: Box::new(move || {
                            feed.update(|feed| feed.remove(item.get_untracked().id()))
                        }),
                        image: Box::new(|_| view().into_element()),
                        replies: Box::new(|| view().into_element()),
                    },
                )
            },
        ))
        .child(emoji::sendbox(
            editor,
            signal(false),
            computed(|| true),
            None,
            emoji::comment_picker(editor, next),
            emoji::SendEvents {
                send: Box::new(move || {
                    let text = editor.text.get_untracked();
                    if text.trim().is_empty() {
                        return;
                    }
                    let id = format!(
                        "{}-{}",
                        parent.get(),
                        feed.with_untracked(|feed| feed.total)
                    );
                    feed.update(|feed| {
                        feed.accept_sent(Item::Reply(ReplyItem {
                            reply_id: id,
                            user_id: "offline".into(),
                            user_name: "离线测试账号".into(),
                            text,
                            ..Default::default()
                        }))
                    });
                    root.update(|feed| feed.add_reply(&parent.get()));
                    editor.text.set(String::new());
                }),
                pick_image: Box::new(|| {}),
            },
            emoji::SendOptions {
                allowed: computed(|| true),
                review: false,
                previous: Box::new(move || focus.previous(feed).focus()),
                next,
            },
        ))
}
