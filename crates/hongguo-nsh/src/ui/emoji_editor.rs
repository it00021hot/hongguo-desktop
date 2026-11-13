//! The three existing input skins over a genuine document editor.
use nsh_base::{
    rich_editor::{
        model::Node,
        view::{
            EditorSession, NodeView, NodeViewCx, NodeViews, RichEditorClasses, RichEditorStyles,
            rich_editor,
        },
    },
    theme,
};
use nsh_core::{
    element::{ImageSrc, Style, StyleRefinement},
    prelude::*,
};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
#[path = "emoji_line.rs"]
mod line;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum EmojiEditorSkin {
    Body,
    Review,
    Video,
}

pub(crate) fn emoji_editor_views(skin: EmojiEditorSkin) -> NodeViews {
    let font = if skin == EmojiEditorSkin::Video {
        12.0
    } else {
        14.0
    };
    NodeViews::new()
        .with("emoji", Arc::new(EmojiImage(font)))
        .with("mention", Arc::new(Mention))
}
struct EmojiImage(f32);
impl NodeView for EmojiImage {
    fn create(&self, _cx: &NodeViewCx, node: &Node) -> AnyElement {
        let uri = node
            .attrs()
            .get("src")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let code = node
            .attrs()
            .get("wire")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let size = self.0 * 1.2;
        image(ImageSrc::from(uri))
            .label(code)
            .size(move || px(size * font_scale()))
            .mx(px(1.0))
            .object_fit(nsh_core::ResizeMode::Contain)
            .align_baseline()
            .into_element()
    }
}
struct Mention;
impl NodeView for Mention {
    fn create(&self, _cx: &NodeViewCx, node: &Node) -> AnyElement {
        let name = node
            .attrs()
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        text(format!("@{name}"))
            .text_color(move || theme().color("video-mention").expect("mention token"))
            .into_element()
    }
}

pub(crate) fn emoji_editor(
    session: EditorSession,
    reference: ElementRef,
    placeholder: Memo<SharedString>,
    disabled: Memo<bool>,
    multiline: bool,
    skin: EmojiEditorSkin,
    changed: impl Fn() + Clone + 'static,
) -> (View, Rc<dyn Fn()>) {
    let selected = changed.clone();
    let video = skin == EmojiEditorSkin::Video;
    let height = move || {
        px(if skin == EmojiEditorSkin::Review {
            64.0
        } else if !is_desktop() {
            48.0
        } else if video {
            32.0
        } else {
            36.0
        })
    };
    let editor = rich_editor()
        .editor(session.clone())
        .element_ref(&reference)
        .h_full()
        .min_w(relative(1.0))
        .style_when(multiline, |style| style.w_full())
        .classes(move || classes(multiline))
        .placeholder(move || Some(placeholder.get()))
        .accessibility_label(move || Some(placeholder.get()))
        .editable(move || !disabled.get())
        .styles(move || styles(skin, multiline))
        .on_change(move |_: &_| changed())
        .on_selection_change(move |_: &_| selected());
    let (editor, reveal) = line::mount(editor.into_element(), session, multiline);
    let view = view()
        .min_w(px(0.0))
        .w_full()
        .h(height)
        .min_h(height)
        .border(move || px(if video { 0.0 } else { 1.0 }))
        .border_color(move || theme().input)
        .rounded(move || {
            px(if video {
                0.0
            } else {
                (theme().radius.0 - 2.0).max(0.0)
            })
        })
        .bg(move || {
            if skin == EmojiEditorSkin::Review {
                theme().muted.opacity(0.5)
            } else {
                theme().background.opacity(0.0)
            }
        })
        .opacity(move || if disabled.get() { 0.45_f32 } else { 1.0_f32 })
        .child(editor);
    (view, reveal)
}

fn classes(multiline: bool) -> RichEditorClasses {
    let mut classes = RichEditorClasses::default();
    if !multiline {
        classes.root = Some("w-max min-w-full whitespace-nowrap".into());
        classes.content = Some("w-max min-w-full whitespace-nowrap".into());
    }
    classes
}

fn styles(skin: EmojiEditorSkin, multiline: bool) -> RichEditorStyles {
    let video = skin == EmojiEditorSkin::Video;
    let color = move || {
        if video {
            theme().color("video-controls").expect("video controls")
        } else {
            theme().foreground
        }
    };
    let font = if video { 12.0 } else { 14.0 };
    let line = if video { 16.0 } else { 20.0 };
    let root: Style = StyleRefinement::default()
        .text_size(px(font))
        .line_height(px(line))
        .font_weight(FontWeight::NORMAL)
        .text_color(color)
        .into();
    let content: Style = StyleRefinement::default()
        .px(px(if video { 0.0 } else { 12.0 }))
        .py(px(if video || multiline { 8.0 } else { 7.0 }))
        .min_h(px(line))
        .into();
    let paragraph: Style = StyleRefinement::default()
        .m_0()
        .text_size(px(font))
        .line_height(px(line))
        .font_weight(FontWeight::NORMAL)
        .text_color(color)
        .into();
    let mut styles = RichEditorStyles::default();
    styles.root = Some(root.into());
    styles.content = Some(content.into());
    let placeholder = StyleRefinement::default().opacity(0.5).text_color(color);
    let placeholder = if multiline {
        placeholder
    } else {
        placeholder
            .number_of_lines(1)
            .text_overflow(nsh_core::EllipsizeMode::Tail)
    };
    styles.placeholder = Some(Style::from(placeholder).into());
    styles.caret = Some(Style::from(StyleRefinement::default().bg(color)).into());
    styles.nodes = Some(nsh_base::rich_editor::view::PartStyles(BTreeMap::from([(
        "paragraph".into(),
        paragraph,
    )])));
    styles
}
