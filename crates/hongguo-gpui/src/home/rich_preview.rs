//! 输入区的「所见即所得」预览：表情图 + 蓝色 @名字。
//!
//! **为什么需要**：Tauri 版的弹幕/评论输入框是 contentEditable（`RichEmojiInput`），
//! 表情以 `<img>` 内联、提及以 chip 内联。GPUI 的 `Input` 是**纯文本编辑器**，
//! 没有内联图片的能力——输入框里只能看到 `[名字]` 这样的代码。本模块给输入框
//! 配一条预览：同一段文本按弹幕层/评论正文那套分段规则渲染出来（表情图 + 蓝色
//! @名字），发出去之前就能看见最终长什么样。
//!
//! 预览只在文本里**真的有表情或提及**时出现（否则输入框已经显示全部内容，
//! 再重复一遍是噪音）。

use gpui_kit::div;
use gpui_kit::prelude::*;

use super::HomeView;

impl HomeView {
    /// 文本里是否含「可渲染的表情或提及」：有才值得开预览。
    pub(in crate::home) fn has_rich_segment(text: &str) -> bool {
        if text.contains('\u{FFFC}') {
            return true;
        }
        crate::danmaku_emoji::parse_emoji_segments(text)
            .iter()
            .any(|seg| matches!(seg, crate::danmaku_emoji::EmojiSegment::Emoji(_)))
    }

    /// 表情/提及预览行（`None` = 没什么可渲染的，不占地方）。
    pub(in crate::home) fn render_rich_preview(&self, text: &str) -> Option<gpui_kit::AnyElement> {
        if text.trim().is_empty() || !Self::has_rich_segment(text) {
            return None;
        }
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(gpui_kit::rgba(0xffffffcc));
        for element in self.rich_children(text) {
            row = row.child(element);
        }
        Some(row.into_any_element())
    }
}
