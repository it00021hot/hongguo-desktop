use super::{
    model::{Feed, Item},
    row_view,
};
use hongguo_api::danmaku::CommentItem;
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[component]
fn ClickHost() -> impl IntoElement {
    let feed = signal(Feed::default());
    feed.update(|feed| {
        feed.accept_sent(Item::Comment(CommentItem {
            comment_id: "zero".into(),
            user_name: "Reader".into(),
            text: "Actual comment".into(),
            digg_count: 4,
            reply_count: 0,
            ..Default::default()
        }))
    });
    view().flex_1().child(each(
        move || feed.get().items,
        |item: &Item| item.id().to_owned(),
        move |item, _| {
            let error = signal(SharedString::default());
            let replies = signal(false);
            row_view::row(
                row_view::State {
                    item,
                    pending: signal(false),
                    error,
                    replies,
                    editing: signal(false),
                    mine: computed(|| false),
                    reviews: false,
                    reply_stop: element_ref(),
                    delete_stop: element_ref(),
                },
                row_view::Events {
                    like: Box::new(move || {
                        super::actions::like(
                            feed,
                            item.get_untracked().id().to_owned(),
                            false,
                            computed(|| None),
                            signal(false),
                            error,
                            |_, _| async {
                                panic!("anonymous action must not send an API request")
                            },
                        )
                    }),
                    reply: Box::new(move || replies.update(|open| *open = !*open)),
                    toggle_replies: Box::new(move || replies.update(|open| *open = !*open)),
                    delete: Box::new(|| {}),
                    image: Box::new(|_| view().into_element()),
                    replies: Box::new(|| view().child(text("Loaded reply")).into_element()),
                },
            )
        },
    ))
}

fn start() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<ClickHost>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(380.0), px(520.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    config.environment.reduce_motion = true;
    Headless::start(config).map_err(|error| error.to_string())
}

#[test]
fn anonymous_like_reaches_the_login_feedback_instead_of_being_a_dead_control() -> Result<(), String>
{
    let mut app = start()?;
    app.wait_for("Actual comment")?;
    app.press("id:comment-zero >> role:button[点赞 4]")?;
    app.wait_for("互动需要先登录（设置页 → 账号）")?;
    Ok(())
}

#[test]
fn zero_replies_do_not_offer_an_expand_action() -> Result<(), String> {
    let mut app = start()?;
    app.wait_for("Actual comment")?;
    assert!(!app.screen().shows("展开 0 条回复"));
    Ok(())
}

#[component]
fn RequestsHost() -> impl IntoElement {
    let feed = signal(Feed::default());
    feed.update(|feed| {
        feed.accept_sent(Item::Comment(CommentItem {
            comment_id: "actual-parent".into(),
            user_name: "Reader".into(),
            text: "Parent".into(),
            digg_count: 4,
            reply_count: 1,
            ..Default::default()
        }))
    });
    let requests = signal(String::new());
    let failure = signal(false);
    let threads = super::threads::Threads::new(
        feed,
        computed(|| Some("account".into())),
        move |parent, cursor| {
            requests.set(format!("load {parent} cursor={cursor}"));
            async move {
                Ok(super::model::Page {
                    summary: None,
                    total: 1,
                    cursor: String::new(),
                    has_more: false,
                    items: vec![Item::Reply(hongguo_api::danmaku::ReplyItem {
                        reply_id: "server-reply".into(),
                        text: "Visible server reply".into(),
                        ..Default::default()
                    })],
                })
            }
        },
    );
    view()
        .flex_1()
        .child(
            crate::ui::button("Fail next like", crate::ui::ButtonVariant::Quiet)
                .on_press(move || failure.set(true)),
        )
        .child(text(move || requests.get()))
        .child(super::list_view::thread_list(
            feed,
            threads,
            view(),
            view(),
            view(),
            "click-list",
            move |entry| match entry.get_untracked() {
                super::threads::Entry::Root(_) => view()
                    .child(each(
                        move || match entry.get() {
                            super::threads::Entry::Root(item) => vec![item],
                            _ => Vec::new(),
                        },
                        |item: &Item| item.id().to_owned(),
                        move |item, _| {
                            let pending = signal(false);
                            let error = signal(SharedString::default());
                            let thread = threads
                                .get(item.get_untracked().id())
                                .expect("root registered");
                            row_view::row(
                                row_view::State {
                                    item,
                                    pending,
                                    error,
                                    replies: thread.expanded,
                                    editing: thread.editing,
                                    mine: computed(|| false),
                                    reviews: false,
                                    reply_stop: element_ref(),
                                    delete_stop: element_ref(),
                                },
                                row_view::Events {
                                    like: Box::new(move || {
                                        threads.run(move || {
                                            super::actions::like(
                                                feed,
                                                item.get_untracked().id().to_owned(),
                                                true,
                                                threads.account,
                                                pending,
                                                error,
                                                move |id, selected| {
                                                    requests.set(format!("like {id} {selected}"));
                                                    let fail = failure.get_untracked();
                                                    feed.update(|feed| {
                                                        feed.add_reply("actual-parent")
                                                    });
                                                    async move {
                                                        if fail {
                                                            Err("Like request failed".into())
                                                        } else {
                                                            Ok(())
                                                        }
                                                    }
                                                },
                                            )
                                        })
                                    }),
                                    reply: Box::new(move || {
                                        thread.editing.update(|value| *value = !*value)
                                    }),
                                    toggle_replies: Box::new(move || {
                                        thread.expanded.update(|value| *value = !*value)
                                    }),
                                    delete: Box::new(|| {}),
                                    image: Box::new(|_| view().into_element()),
                                    replies: Box::new(move || {
                                        view()
                                            .child(when(thread.editing, || text("Reply editor")))
                                            .into_element()
                                    }),
                                },
                            )
                        },
                    ))
                    .into_element(),
                super::threads::Entry::Reply { .. } => text("Visible server reply").into_element(),
                super::threads::Entry::Footer(_) => view().into_element(),
            },
        ))
}

fn start_requests() -> Result<Headless, String> {
    let mut config = EngineConfig::new(
        Root::of::<RequestsHost>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.viewport = nsh_engine::Size::new(px(380.0), px(520.0));
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    Headless::start(config).map_err(|error| error.to_string())
}

#[test]
fn real_like_action_updates_and_rolls_back_without_losing_concurrent_reply_counts()
-> Result<(), String> {
    let mut app = start_requests()?;
    app.wait_for("Parent")?;
    app.press("role:button[点赞 4]")?;
    app.wait_for("like actual-parent true")?;
    app.wait_for("role:button[已点赞 5]")?;
    app.press("Fail next like")?;
    app.press("role:button[已点赞 5]")?;
    app.wait_for("Like request failed")?;
    app.wait_for("role:button[已点赞 5]")?;
    app.wait_for("展开 3 条回复")?;
    Ok(())
}

#[test]
fn expand_loads_the_correct_parent_and_collapse_is_independent_of_reply_editor()
-> Result<(), String> {
    let mut app = start_requests()?;
    app.wait_for("Parent")?;
    assert!(!app.screen().shows("Visible server reply"));
    app.press("role:button[回复]")?;
    app.wait_for("Reply editor")?;
    assert!(!app.screen().shows("Visible server reply"));
    app.press("展开 1 条回复")?;
    app.wait_for("load actual-parent cursor=")?;
    app.wait_for("Visible server reply")?;
    app.press("收起回复")?;
    app.wait_for("Reply editor")?;
    assert!(!app.screen().shows("Visible server reply"));
    Ok(())
}

#[component]
fn RetainedHost() -> impl IntoElement {
    let feed = signal(Feed::default());
    feed.update(|feed| {
        feed.accept_sent(Item::Comment(CommentItem {
            comment_id: "retained".into(),
            text: "Retained row".into(),
            digg_count: 4,
            ..Default::default()
        }))
    });
    let threads =
        super::threads::Threads::new(feed, computed(|| Some("account".into())), |_, _| async {
            unreachable!()
        });
    let (send, receive) = tokio::sync::oneshot::channel::<()>();
    let send = stored(Some(send));
    let receive = stored(Some(receive));
    let shown = signal(true);
    view()
        .child(
            crate::ui::button("Recycle row", crate::ui::ButtonVariant::Quiet)
                .on_press(move || shown.set(false)),
        )
        .child(
            crate::ui::button("Return row", crate::ui::ButtonVariant::Quiet)
                .on_press(move || shown.set(true)),
        )
        .child(
            crate::ui::button("Finish response", crate::ui::ButtonVariant::Quiet).on_press(
                move || {
                    send.with(|sender| sender.take()).unwrap().send(()).unwrap();
                },
            ),
        )
        .child(when(shown, move || {
            view().child(each(
                move || feed.get().items,
                |item: &Item| item.id().to_owned(),
                move |item, _| {
                    let state = threads.action(item.get_untracked().id());
                    row_view::row(
                        row_view::State {
                            item,
                            pending: state.pending,
                            error: state.error,
                            replies: signal(false),
                            editing: signal(false),
                            mine: computed(|| false),
                            reviews: false,
                            reply_stop: element_ref(),
                            delete_stop: element_ref(),
                        },
                        row_view::Events {
                            like: Box::new(move || {
                                threads.run(move || {
                                    super::actions::like(
                                        feed,
                                        item.get_untracked().id().to_owned(),
                                        true,
                                        threads.account,
                                        state.pending,
                                        state.error,
                                        move |_, _| {
                                            let receive = receive.with(Option::take).unwrap();
                                            async move {
                                                receive.await.unwrap();
                                                Err("Retained failure".into())
                                            }
                                        },
                                    )
                                })
                            }),
                            reply: Box::new(|| {}),
                            toggle_replies: Box::new(|| {}),
                            delete: Box::new(|| {}),
                            image: Box::new(|_| view().into_element()),
                            replies: Box::new(|| view().into_element()),
                        },
                    )
                },
            ))
        }))
}

#[test]
fn like_response_survives_row_recycling_and_restores_feedback_on_return() -> Result<(), String> {
    let mut config = EngineConfig::new(
        Root::of::<RetainedHost>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../../../theme.json"))).unwrap()),
    );
    config.environment.os = nsh_core::platform::Os::MacOs;
    config.environment.locale = nsh_core::platform::Locale::parse("zh-CN").unwrap();
    let mut app = Headless::start(config).map_err(|error| error.to_string())?;
    app.wait_for("Retained row")?;
    app.press("role:button[点赞 4]")?;
    app.wait_for("role:button[已点赞 5]")?;
    app.press("Recycle row")?;
    app.press("Finish response")?;
    app.press("Return row")?;
    app.wait_for("Retained failure")?;
    app.wait_for("role:button[点赞 4]")?;
    assert!(
        !app.screen()
            .accessibility_state("role:button[点赞 4]")?
            .disabled
    );
    Ok(())
}
