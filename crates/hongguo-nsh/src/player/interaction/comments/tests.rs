use super::model::{Feed, Item, Page};
use hongguo_api::danmaku::{CommentItem, ReplyItem};

fn comment(id: &str, text: &str) -> Item {
    Item::Comment(CommentItem {
        comment_id: id.into(),
        text: text.into(),
        ..Default::default()
    })
}

#[test]
fn delayed_index_does_not_remove_the_full_send_echo_or_duplicate_rows() {
    let mut feed = Feed::default();
    let echo = Item::Comment(CommentItem {
        comment_id: "new".into(),
        user_id: "me".into(),
        user_name: "Milo".into(),
        avatar: "https://example.test/avatar".into(),
        text: "Sent".into(),
        ..Default::default()
    });
    feed.accept_sent(echo.clone());
    feed.append(
        Page {
            items: vec![comment("old", "Older")],
            summary: None,
            total: 8,
            cursor: "20".into(),
            has_more: true,
        },
        "",
    );
    assert_eq!(feed.items[0], echo);
    assert_eq!(feed.total, 9);
    feed.append(
        Page {
            items: vec![comment("old", "Older"), echo.clone()],
            summary: None,
            total: 9,
            cursor: "40".into(),
            has_more: true,
        },
        "20",
    );
    assert_eq!(feed.items, vec![echo, comment("old", "Older")]);
    assert_eq!(feed.total, 9);
}

#[test]
fn a_non_advancing_cursor_cannot_request_the_same_page_forever() {
    let mut feed = Feed::default();
    feed.append(
        Page {
            items: vec![],
            summary: None,
            total: 12,
            cursor: "20".into(),
            has_more: true,
        },
        "20",
    );
    assert!(!feed.has_more);
}

#[test]
fn replies_keep_their_server_identity_and_reply_target() {
    let mut feed = Feed::default();
    let echo = Item::Reply(ReplyItem {
        reply_id: "r".into(),
        user_id: "u".into(),
        user_name: "Reader".into(),
        reply_to_reply_id: "earlier".into(),
        reply_to_name: "Writer".into(),
        text: "Hello".into(),
        ..Default::default()
    });
    feed.accept_sent(echo.clone());
    assert_eq!(feed.items, vec![echo]);
    assert_eq!(feed.total, 1);
}

#[test]
fn comment_like_rollback_does_not_overwrite_a_concurrent_reply_count() {
    let mut feed = Feed::default();
    feed.accept_sent(comment("c", "Hello"));
    let before = feed.toggle_like("c").unwrap();
    feed.add_reply("c");
    feed.restore_like("c", before);
    let Item::Comment(item) = &feed.items[0] else {
        panic!("comment expected")
    };
    assert!(!item.user_digg);
    assert_eq!(item.digg_count, 0);
    assert_eq!(item.reply_count, 1);
}
