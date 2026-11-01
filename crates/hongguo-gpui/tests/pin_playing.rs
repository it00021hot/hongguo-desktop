//! 在播钉住断言（Tauri `pinned-playing.test.ts` 同规则）。
//!
//! 纯逻辑，默认构建即可跑（不依赖 gpui test-support）：源文件级引入，
//! 因为壳是纯 bin（模块内 `#[cfg(test)]` 不会执行）。

#[path = "../src/home/pin_playing.rs"]
mod pin_playing;

use pin_playing::{PinItem, pin_playing_front};

type Body = &'static str;

fn item(key: &str, body: Body) -> PinItem<Body> {
    PinItem::new(key, body)
}

fn ids(items: &[PinItem<Body>]) -> Vec<&str> {
    items.iter().map(|p| p.key.as_str()).collect()
}

fn pin(
    items: Vec<PinItem<Body>>,
    pin_hold: bool,
    playing_id: Option<&str>,
    placeholder: Option<PinItem<Body>>,
) -> Vec<PinItem<Body>> {
    pin_playing_front(items, pin_hold, playing_id, || placeholder)
}

// 回归锁（Tauri 同款复现）：流里还含在播剧时也必须钉到最前——旧逻辑此处
// 不钉，换批后游标指到别的剧，follow_cursor 把播放目标拽走（「刷剧被自动
// 切到别的剧」）。
#[test]
fn pins_playing_even_when_still_in_list() {
    let out = pin(
        vec![item("a", "a"), item("p", "在播"), item("b", "b")],
        true,
        Some("p"),
        None,
    );
    assert_eq!(ids(&out), ["p", "a", "b"]);
    assert_eq!(out[0].body, "在播", "原样复用条目，载荷不降级成占位");
}

#[test]
fn gate_closed_means_no_pin() {
    let items = vec![item("a", "a"), item("p", "p")];
    let out = pin(items.clone(), false, Some("a"), Some(item("x", "x")));
    assert_eq!(ids(&out), ["a", "p"]);
}

#[test]
fn no_playing_means_no_pin() {
    let items = vec![item("a", "a")];
    let out = pin(items.clone(), true, None, Some(item("x", "x")));
    assert_eq!(ids(&out), ["a"]);
}

#[test]
fn pins_placeholder_when_playing_swapped_out() {
    let out = pin(
        vec![item("a", "a"), item("b", "b")],
        true,
        Some("p"),
        Some(item("p", "占位")),
    );
    assert_eq!(ids(&out), ["p", "a", "b"]);
}

#[test]
fn empty_list_still_pins_placeholder() {
    let out = pin(vec![], true, Some("p"), Some(item("p", "占位")));
    assert_eq!(ids(&out), ["p"]);
}

#[test]
fn missing_placeholder_means_no_pin() {
    let items = vec![item("a", "a")];
    let out = pin(items.clone(), true, Some("p"), None);
    assert_eq!(ids(&out), ["a"]);
}

#[test]
fn placeholder_not_called_when_playing_in_list() {
    let called = std::cell::Cell::new(false);
    pin_playing_front(vec![item("p", "在播")], true, Some("p"), || {
        called.set(true);
        Some(item("p", "占位"))
    });
    assert!(!called.get(), "流里已有在播剧，不该花占位构造");
}

#[test]
fn rest_keep_relative_order_without_duplicates() {
    let out = pin(
        vec![
            item("a", "a"),
            item("p", "在播"),
            item("b", "b"),
            item("c", "c"),
        ],
        true,
        Some("p"),
        None,
    );
    assert_eq!(ids(&out), ["p", "a", "b", "c"]);
}
