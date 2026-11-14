//! Offline adapter for the production picker, editor and attachment presentation.
use super::{
    Attachment, Editor, SYSTEM, Scene,
    attachment::LocalImage,
    panel::{self, Intent, State},
    remote_model::{Feed, Tab},
};
use crate::{i18n::tr, ui};
use hongguo_api::emoticon::Emoticon;
use nsh_core::prelude::*;

pub(crate) fn fixture() -> impl IntoElement {
    let editor = Editor::new(signal("你好[微笑]".into()), 200);
    let attachment = signal(None::<Attachment>);
    let open = signal(false);
    let feed = signal(Feed::default());
    let mine = signal(Vec::<Emoticon>::new());
    let notice = signal(SharedString::default());
    let output = signal(String::new());
    let fail = signal(false);
    let next = element_ref();
    let previous = element_ref();
    let state = State {
        feed,
        notice,
        busy: computed(|| false),
        failed: computed(|| false),
    };
    let fill = move || {
        let current = feed.get_untracked().query;
        let items = match current.tab {
            Tab::System => vec![],
            Tab::Own => mine.get_untracked(),
            Tab::Search if current.keyword.trim().is_empty() => vec![],
            Tab::Search | Tab::ThirdParty => (0..18).map(|at| sticker(at, "测试表情")).collect(),
        };
        feed.update(|feed| {
            feed.query.account = Some("offline".into());
            feed.items = items;
        });
    };
    let action = move |intent| match intent {
        Intent::Insert(code) => {
            if let Err(error) = editor.insert(code) {
                notice.set(error);
            }
        }
        Intent::Tab(tab) => {
            feed.update(|feed| feed.select(tab, String::new()));
            fill();
        }
        Intent::Search(value) => {
            feed.update(|feed| feed.select(Tab::Search, value));
            fill();
        }
        Intent::Pick(image, tab) => {
            attachment.set(Some(Attachment::Remote(
                image,
                if tab == Tab::Own {
                    Scene::Own
                } else {
                    Scene::ThirdParty
                },
            )));
            editor.reference.focus();
        }
        Intent::Collect(image, remove) => {
            if remove {
                mine.update(|items| items.retain(|item| item.id != image.id));
            } else {
                mine.update(|items| {
                    if !items.iter().any(|item| item.id == image.id) {
                        items.push(image);
                    }
                });
            }
            notice.set(tr(if remove {
                "player.emoticon.removeDone"
            } else {
                "player.emoticon.addDone"
            }));
            fill();
        }
        Intent::Upload => {
            mine.update(|items| items.push(sticker(100, "上传的表情")));
            notice.set(tr("player.emoticon.uploadDone"));
            fill();
        }
        Intent::More | Intent::Retry | Intent::Login => {}
    };
    view()
        .test_id("emoji-fixture")
        .gap_3()
        .child(text("离线表情与附件验证"))
        .child(super::render(
            "欢迎[微笑] @阿明\u{fffc} 未知[未收录]".to_owned(),
            ui::EmojiTextStyle::Body,
        ))
        .child(
            ui::button("模拟发送失败", ui::ButtonVariant::Quiet)
                .element_ref(&previous)
                .on_press(move || fail.set(true)),
        )
        .child(super::sendbox(
            editor,
            signal(false),
            computed(|| true),
            Some(attachment),
            ui::emoji_picker(
                open,
                ui::emoji_trigger(
                    ui::Icon::Smile,
                    move || tr("player.danmakuEmoji"),
                    ui::EmojiTextStyle::Body,
                )
                .element_ref(&next),
                panel::panel(state, true, action),
                move || editor.reference.focus(),
            ),
            super::SendEvents {
                send: Box::new(move || {
                    if fail.get_untracked() {
                        notice.set("离线模拟：发送失败，草稿已保留".into());
                        fail.set(false);
                        return;
                    }
                    output.set(format!(
                        "已发送：{}{}",
                        editor.text.get_untracked(),
                        if attachment.with_untracked(Option::is_some) {
                            "（含图片）"
                        } else {
                            ""
                        }
                    ));
                    editor.text.set(String::new());
                    attachment.set(None);
                    notice.set(SharedString::default());
                }),
                pick_image: Box::new(move || {
                    attachment.set(Some(Attachment::Local(LocalImage {
                        bytes: include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/../../assets/emoji/e01.webp"
                        ))
                        .to_vec()
                        .into(),
                        filename: "offline.webp".into(),
                        width: 128,
                        height: 128,
                        format: "webp".into(),
                        preview: SYSTEM[0].uri.into(),
                    })))
                }),
            },
            super::SendOptions {
                allowed: computed(|| true),
                review: false,
                reply: false,
                previous: Box::new(move || previous.focus()),
                next,
            },
        ))
        .child(text(move || output.get()).test_id("emoji-fixture-output"))
        .child(ui::status(notice))
}

fn sticker(index: usize, name: &str) -> Emoticon {
    Emoticon {
        id: Some(format!("offline-{index}")),
        image_name: Some(format!("{name}{index}")),
        web_uri: SYSTEM[index % SYSTEM.len()].uri.into(),
        image_type: 1,
        width: 128,
        height: 128,
        ..Default::default()
    }
}
