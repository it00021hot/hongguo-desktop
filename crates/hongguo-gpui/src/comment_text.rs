//! 评论类文本（评论正文 / 回复正文 / 剧评回复）的富文本解析与渲染。
//!
//! ## 为什么不能把文本拆成多个 flex item
//!
//! GPUI 的文本只有拿到**确定宽度**才折行：`TextLayout::layout` 的 `wrap_width`
//! 取 `known_dimensions.width`，取不到才看 `available_space.width` 是不是
//! `Definite`，两者都没有就按 max-content 排版（gpui-pre 0.3.9
//! `elements/text.rs`）。而 flex 布局里每个 item 的 flex base size 都按
//! max-content 量（taffy 0.13 `compute/flexbox.rs` 的
//! `determine_flex_base_size`），文本一旦被拆成多个 item（提及、表情各自成件），
//! 整段中文不折行、横向撑出面板——这正是评论文字飞出面板右边界的原因。
//!
//! 所以这里把**连续文本合成一段**，用 `StyledText` 的高亮区间区分颜色，整段作为
//! 单个文本节点渲染；外层用列容器（列会 stretch 子件）给出确定宽度 → 正常换行。
//! 表情图仍是独立元素（GPUI 没有行内图），表情码的位置决定文本段的切分。
//!
//! 口径对齐 Tauri 壳 `CommentText` / `EmojiText`：提及段 `@名字\u{FFFC}` 染蓝
//! （开头的提及前缀命中 AI bot 时追加琥珀「Ai」后缀），`[名字]` 表情码换内联
//! 表情图。

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{HighlightStyle, Hsla, RenderImage, StyledText, div};

/// 表情图边长（px）——与 Tauri 的 `size-[1.15em]` 同量级。
const EMOJI_SIZE: f32 = 18.0;

/// 提及高亮色（官方蓝；播放器皮肤域固定色值，属红线 11 豁免）。
pub(crate) fn mention_color() -> Hsla {
    gpui_kit::rgb(0x38bdf8).into()
}

/// AI 徽标色（琥珀；Tauri 的琥珀→粉渐变在 GPUI 用单色近似）。
pub(crate) fn ai_color() -> Hsla {
    gpui_kit::rgb(0xfbbf24).into()
}

/// 富文本片段：文本段与表情图段按出现顺序交替。
pub(crate) enum Segment {
    /// 文本段（可含提及高亮）。
    Text(TextPart),
    /// 表情图段。
    Emoji(Arc<RenderImage>),
}

/// 文本段：展示文本 + 高亮区间。
pub(crate) struct TextPart {
    /// 展示文本（表情码与提及占位符已剔除）。
    pub(crate) text: String,
    /// 高亮区间（字节区间，按起点升序、不重叠）。
    pub(crate) highlights: Vec<(Range<usize>, HighlightStyle)>,
}

impl TextPart {
    /// 追加纯文本。
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// 追加高亮文本（并记录区间）。
    fn push_highlight(&mut self, text: &str, style: HighlightStyle) {
        let start = self.text.len();
        self.text.push_str(text);
        self.highlights.push((start..self.text.len(), style));
    }

    /// 追加正文：提及段 `@名字\u{FFFC}` 染蓝、占位符剔除，其余原样。
    fn push_mentions(&mut self, text: &str) {
        for piece in crate::emoji_scan::split_mention_segments(text) {
            match piece {
                crate::emoji_scan::MentionPiece::Text(text) => self.push(&text),
                crate::emoji_scan::MentionPiece::Mention(name) => {
                    self.push_highlight(&format!("@{name}"), mention_style());
                    // Tauri `EmojiText` 的提及段带 `mr-1` 间隙，这里用空格顶替。
                    self.push(" ");
                }
            }
        }
    }
}

/// 提及高亮样式（官方蓝）。
fn mention_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(mention_color()),
        ..Default::default()
    }
}

/// AI 徽标样式（琥珀加粗）。
fn ai_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(ai_color()),
        font_weight: Some(gpui_kit::FontWeight::SEMIBOLD),
        ..Default::default()
    }
}

/// 正文解析器：按出现顺序拼文本段与表情图段。
struct Builder {
    segments: Vec<Segment>,
    part: TextPart,
}

impl Builder {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            part: TextPart {
                text: String::new(),
                highlights: Vec::new(),
            },
        }
    }

    /// 结束当前文本段（表情图前）。
    fn flush(&mut self) {
        if self.part.text.is_empty() {
            return;
        }
        self.segments.push(Segment::Text(std::mem::replace(
            &mut self.part,
            TextPart {
                text: String::new(),
                highlights: Vec::new(),
            },
        )));
    }

    /// 追加正文：表情码换表情图段，提及段染蓝。
    fn push_body(&mut self, text: &str) {
        for segment in crate::danmaku_emoji::parse_emoji_segments(text) {
            match segment {
                crate::danmaku_emoji::EmojiSegment::Text(text) => self.part.push_mentions(&text),
                crate::danmaku_emoji::EmojiSegment::Emoji(image) => {
                    self.flush();
                    self.segments.push(Segment::Emoji(image));
                }
            }
        }
    }

    fn finish(mut self) -> Vec<Segment> {
        self.flush();
        self.segments
    }
}

/// 顶层评论正文：开头的 `@名字\u{FFFC}` 提及前缀染蓝（命中 AI bot 时追加琥珀
/// 「Ai」后缀），正文里的提及段同样染蓝，`[名字]` 表情码换内联表情图。
pub(crate) fn comment_body(text: &str, ai_name: Option<&str>) -> Vec<Segment> {
    let mut builder = Builder::new();
    if let Some((name, rest)) = parse_mention_prefix(text) {
        builder
            .part
            .push_highlight(&format!("@{name}"), mention_style());
        if ai_name == Some(name.as_str()) {
            builder.part.push(" ");
            builder.part.push_highlight("Ai", ai_style());
        }
        // Tauri `CommentText` 的提及前缀带 `mr-1` 间隙，这里用空格顶替。
        builder.part.push(" ");
        builder.push_body(rest);
    } else {
        builder.push_body(text);
    }
    builder.finish()
}

/// 回复正文：`reply_to` 非空时前置「回复 @某人：」前缀（@某人 染蓝），否则只渲染
/// 正文——官方手机 App 与 Tauri 壳都是后者（回复评论本条时没有前缀）。
///
/// `prefix_key` 是前缀词条键（播放器 `player.comments.replyToPrefixName`、剧评
/// `detail.replyToPrefixName`）；`mention` 为 `Some` 时 @某人 染蓝（播放器皮肤
/// 域），`None` 表示沿用正文颜色（剧评页走语义色）。
pub(crate) fn reply_body(
    text: &str,
    reply_to: &str,
    prefix_key: &str,
    mention: Option<Hsla>,
) -> Vec<Segment> {
    let mut builder = Builder::new();
    if !reply_to.is_empty() {
        push_reply_prefix(&mut builder.part, prefix_key, reply_to, mention);
    }
    builder.push_body(text);
    builder.finish()
}

/// 回复前缀「回复 @某人：」：@某人 染蓝（`mention` 为 `None` 时沿用正文色）。
fn push_reply_prefix(part: &mut TextPart, prefix_key: &str, name: &str, mention: Option<Hsla>) {
    let prefix = crate::i18n::tf(prefix_key, &[("name", name)]);
    // 词条形如「回复 @{name}：」——@ 与名字一起染蓝（Tauri 同款）；词条没带 @
    // 时退回只染名字。
    let needle = format!("@{name}");
    let (begin, end) = match prefix.find(&needle) {
        Some(at) => (at, at + needle.len()),
        None => match prefix.find(name) {
            Some(start) => (start, start + name.len()),
            None => {
                part.push(&prefix);
                return;
            }
        },
    };
    part.push(&prefix[..begin]);
    part.push_highlight(
        &prefix[begin..end],
        HighlightStyle {
            color: mention,
            ..Default::default()
        },
    );
    part.push(&prefix[end..]);
}

/// 渲染富文本片段。
///
/// 文本段包一层列容器：列会 stretch 子件，文本节点因此拿到确定宽度而折行（放在
/// 行容器里，文本按 max-content 排版，永远不折）。
pub(crate) fn render(segments: &[Segment]) -> gpui_kit::AnyElement {
    let mut row = div().flex().flex_wrap().items_start().min_w_0();
    for segment in segments {
        row = row.child(match segment {
            Segment::Text(part) => v_flex()
                .min_w_0()
                .child(
                    StyledText::new(gpui_kit::SharedString::from(part.text.clone()))
                        .with_highlights(
                            part.highlights
                                .iter()
                                .map(|(range, style)| (range.clone(), *style)),
                        ),
                )
                .into_any_element(),
            Segment::Emoji(image) => gpui_kit::img(gpui_kit::ImageSource::Render(image.clone()))
                .size(gpui_kit::px(EMOJI_SIZE))
                .object_fit(gpui_kit::ObjectFit::Contain)
                .flex_shrink_0()
                .into_any_element(),
        });
    }
    row.into_any_element()
}

/// 提取 `@名字\u{FFFC}` 提及前缀（Tauri `CommentText` 的
/// `/^@([^\s\u{FFFC}]+)\u{FFFC}\s*/` 同口径），返回（名字, 余下正文）。
fn parse_mention_prefix(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix('@')?;
    let end = rest.find('\u{FFFC}')?;
    let name = &rest[..end];
    if name.is_empty() || name.chars().any(|c| c.is_whitespace() || c == '@') {
        return None;
    }
    let after = &rest[end + '\u{FFFC}'.len_utf8()..];
    Some((name.to_string(), after.trim_start()))
}
