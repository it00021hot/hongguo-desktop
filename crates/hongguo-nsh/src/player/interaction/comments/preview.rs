//! Offline data, production thread resources, actions, virtual rows and editor.
use super::{
    actions, list_view,
    model::{Feed, Item, Page},
    review_form,
    review_model::ReviewSummary,
    row_view,
    threads::{Entry, ReplyThread, Threads},
};
use crate::{emoji, ui};
use hongguo_api::danmaku::{CommentItem, CommentTagStat, ReplyItem};
use nsh_core::{keymap::When, prelude::*};

pub(crate) fn reviews_preview_with_header(header: View) -> View {
    let feed = seed(true);
    install_empty_shortcut(feed);
    let threads = replies(feed);
    let editor = emoji::Editor::new(signal(String::new()), 500);
    let stars = signal(0);
    let output = signal(SharedString::default());
    let next_id = signal(100_u32);
    let form = review_form::form(
        editor,
        stars,
        signal(false),
        computed(|| true),
        review_form::Events {
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
    let header = view()
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
    render(
        feed,
        threads,
        true,
        header,
        ui::detail_section().pb(px(144.0)).child(ui::status(output)),
    )
}

pub(crate) fn comments_preview() -> View {
    let geometry = ui::PlayerGeometry {
        source: signal((720.0, 1280.0)),
        viewport: signal((0.0, 0.0)),
        comment_anchor: element_ref(),
        comment_trigger: element_ref(),
    };
    provide_context(geometry);
    let open = signal(true);
    on_key("c", move |_| open.set(true));
    let stage = crate::player::stage::fixture(
        nsh_core::element::ImageSrc::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/nsh-layout-fixtures/player-frame.png"
        )),
        || {},
    );
    view()
        .flex_1()
        .child(stage)
        .child(ui::comment_panel(open, signal(80), geometry, move || {
            let feed = seed(false);
            install_empty_shortcut(feed);
            let threads = replies(feed);
            let editor = emoji::Editor::new(signal(String::new()), 200);
            let attachment = signal(None::<emoji::Attachment>);
            let next = element_ref();
            let pagination = view().child(
                super::more_button(feed, false)
                    .on_press(move || {
                        feed.update(|feed| feed.total = feed.items.len() as i64);
                    })
                    .style_when(
                        move || feed.with(|feed| feed.total <= feed.items.len() as i64),
                        |style| style.hidden(),
                    ),
            );
            view()
                .flex_1()
                .min_h_0()
                .child(render(feed, threads, false, view(), pagination))
                .child(
                    view()
                        .p_3()
                        .border_t_1()
                        .border_color(move || {
                            nsh_base::theme()
                                .color("video-controls")
                                .expect("controls")
                                .opacity(0.1)
                        })
                        .child(emoji::sendbox(
                            editor,
                            signal(false),
                            computed(|| true),
                            Some(attachment),
                            emoji::comment_picker(editor, next),
                            emoji::SendEvents {
                                send: Box::new(move || {
                                    let text = editor.text.get_untracked();
                                    if text.trim().is_empty()
                                        && attachment.get_untracked().is_none()
                                    {
                                        return;
                                    }
                                    let images = match attachment.get_untracked() {
                                        Some(emoji::Attachment::Remote(image, _)) => vec![image],
                                        _ => Vec::new(),
                                    };
                                    feed.update(|feed| {
                                        feed.accept_sent(Item::Comment(CommentItem {
                                            comment_id: "comment-sent".into(),
                                            user_id: "offline".into(),
                                            user_name: "离线测试账号".into(),
                                            text,
                                            images,
                                            ..Default::default()
                                        }))
                                    });
                                    editor.text.set(String::new());
                                    attachment.set(None);
                                }),
                                pick_image: Box::new(move || {
                                    attachment.set(Some(emoji::Attachment::Remote(
                                        hongguo_api::emoticon::Emoticon {
                                            web_uri: emoji::SYSTEM[0].uri.into(),
                                            width: 128,
                                            height: 128,
                                            ..Default::default()
                                        },
                                        emoji::Scene::Own,
                                    )))
                                }),
                            },
                            emoji::SendOptions {
                                allowed: computed(|| true),
                                review: false,
                                reply: false,
                                previous: Box::new(move || geometry.comment_trigger.focus()),
                                next,
                            },
                        )),
                )
        }))
}

fn seed(review: bool) -> Signal<Feed> {
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
                        format!("这是第{}条离线评论，推荐大家看看。", index + 1)
                    },
                    score: if review { "7".into() } else { String::new() },
                    score_suffix_text: if review {
                        "观看1小时后点评".into()
                    } else {
                        String::new()
                    },
                    create_time: 1_794_355_200_000,
                    reply_count: i64::from(index == 0),
                    digg_count: 12,
                    ..Default::default()
                })
            })
            .collect();
        feed.total = if review { 80 } else { 100 };
        feed.summary = Some(ReviewSummary {
            score: "8.3".into(),
            count: 1001,
            tags: vec![CommentTagStat {
                tag_name: "人物生动".into(),
                count: 26,
            }],
        });
    });
    feed
}

fn replies(feed: Signal<Feed>) -> Threads {
    let threads = Threads::new(
        feed,
        computed(|| Some("offline".into())),
        |parent, _| async move {
            Ok(Page {
                summary: None,
                total: 1,
                cursor: String::new(),
                has_more: false,
                items: vec![Item::Reply(ReplyItem {
                    reply_id: format!("{parent}-reply"),
                    user_id: "reply-reader".into(),
                    user_name: "回复的观众".into(),
                    reply_to_name: "第1位观众".into(),
                    text: "确实很好看[偷笑]".into(),
                    digg_count: 2,
                    ..Default::default()
                })],
            })
        },
    );
    let initialized = stored(false);
    effect(move || {
        if !initialized.get()
            && let Some(thread) = threads.get("review-preview-0")
        {
            initialized.replace(true);
            thread.expanded.set(true);
        }
    });
    threads
}

fn render(feed: Signal<Feed>, threads: Threads, reviews: bool, header: View, footer: View) -> View {
    list_view::thread_list(
        feed,
        threads,
        header,
        footer,
        ui::comment_notice(
            move || {
                crate::i18n::tr(if reviews {
                    "detail.commentsEmpty"
                } else {
                    "player.comments.empty"
                })
            },
            reviews,
        ),
        if reviews {
            "reviews-list"
        } else {
            "comments-list"
        },
        move |entry| {
            let content = match entry.get_untracked() {
                Entry::Root(_) => view().child(each(
                    move || match entry.get() {
                        Entry::Root(item) => vec![item],
                        _ => Vec::new(),
                    },
                    |item: &Item| item.id().to_owned(),
                    move |item, _| fixture_row(item, feed, threads, reviews, None),
                )),
                Entry::Reply { parent, .. } => {
                    let thread = threads.get(&parent).expect("reply parent");
                    let parent = stored(parent);
                    ui::comment_thread_row(
                        each(
                            move || match entry.get() {
                                Entry::Reply { item, .. } => vec![item],
                                _ => Vec::new(),
                            },
                            |item: &Item| item.id().to_owned(),
                            move |item, _| {
                                fixture_row(
                                    item,
                                    thread.feed,
                                    threads,
                                    reviews,
                                    Some((feed, parent)),
                                )
                            },
                        ),
                        reviews,
                    )
                }
                Entry::Footer(parent) => {
                    let footer = super::reply_footer(threads.get(&parent).expect("footer parent"));
                    if reviews {
                        footer
                            .pb_4()
                            .border_b_1()
                            .border_color(move || nsh_base::theme().border)
                    } else {
                        footer
                    }
                }
            };
            if reviews {
                ui::detail_section().child(content).into_element()
            } else {
                view().px_4().child(content).into_element()
            }
        },
    )
}

fn install_empty_shortcut(feed: Signal<Feed>) {
    on_key_with(
        "mod+shift+0",
        KeyOptions {
            when: When::Always,
            repeat: false,
        },
        move |_| {
            let mut empty = Feed::default();
            empty.summary = feed.get_untracked().summary;
            feed.set(empty);
        },
    );
}

fn fixture_row(
    item: ReadSignal<Item>,
    feed: Signal<Feed>,
    threads: Threads,
    reviews: bool,
    parent: Option<(Signal<Feed>, Stored<String>)>,
) -> View {
    let action = threads.action(item.get_untracked().id());
    let own = threads.get(item.get_untracked().id());
    let expanded = own.map_or_else(|| signal(false), |thread| thread.expanded);
    let editing = own.map_or_else(|| signal(false), |thread| thread.editing);
    row_view::row(
        row_view::State {
            item,
            pending: action.pending,
            error: action.error,
            replies: expanded,
            editing,
            mine: computed(move || item.get().user_id() == "offline"),
            reviews,
            reply_stop: element_ref(),
            delete_stop: element_ref(),
        },
        row_view::Events {
            like: Box::new(move || {
                threads.run(move || {
                    actions::like(
                        feed,
                        item.get_untracked().id().to_owned(),
                        true,
                        threads.account,
                        action.pending,
                        action.error,
                        |_, _| async { Ok(()) },
                    )
                })
            }),
            reply: Box::new(move || {
                if let Some(thread) = own {
                    thread.recipient.set(None);
                    thread.editing.update(|value| *value = !*value);
                } else if let Some((_, parent)) = parent
                    && let Some(thread) = threads.get(&parent.get())
                {
                    thread.recipient.set(Some((
                        item.get_untracked().id().to_owned(),
                        item.get_untracked().name().to_owned(),
                    )));
                    thread.editing.set(true);
                }
            }),
            toggle_replies: Box::new(move || expanded.update(|value| *value = !*value)),
            delete: Box::new(move || {
                feed.update(|feed| feed.remove(item.get_untracked().id()));
                if let Some((root, parent)) = parent {
                    root.update(|feed| feed.remove_reply(&parent.get()));
                }
            }),
            image: Box::new(move |image| {
                ui::emoji_comment_image(move || {
                    nsh_core::element::ImageSrc::from(image.get().web_uri)
                })
                .into_element()
            }),
            replies: Box::new(move || {
                own.map_or_else(
                    || view().into_element(),
                    |thread| {
                        reply_form(thread, feed, item.get_untracked().id().to_owned(), reviews)
                            .into_element()
                    },
                )
            }),
        },
    )
}

fn reply_form(thread: ReplyThread, root: Signal<Feed>, parent: String, review: bool) -> View {
    let parent = stored(parent);
    let editor = emoji::Editor::new(signal(String::new()), 200);
    let next = element_ref();
    view().mt_2().child(emoji::sendbox(
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
                let recipient = thread.recipient.get_untracked();
                thread.feed.update(|feed| {
                    feed.accept_sent(Item::Reply(ReplyItem {
                        reply_id: format!("{}-sent-{}", parent.get(), feed.total),
                        user_id: "offline".into(),
                        user_name: "离线测试账号".into(),
                        text,
                        reply_to_name: recipient.map_or_else(String::new, |(_, name)| name),
                        ..Default::default()
                    }))
                });
                root.update(|feed| feed.add_reply(&parent.get()));
                editor.text.set(String::new());
                thread.editing.set(false);
                thread.expanded.set(true);
            }),
            pick_image: Box::new(|| {}),
        },
        emoji::SendOptions {
            allowed: computed(|| true),
            review,
            reply: true,
            previous: Box::new(|| {}),
            next,
        },
    ))
}
