//! 弹幕表情：`[名字]` 代码 → 打包 webp 的解码与渲染分段。
//!
//! 上游文本里表情以 `[名字]` 代码出现（hgplayer 1.1.6 同款，1–8 个非空白
//! 字符），渲染时按映射表换打包的 webp 图——弹幕/评论同源。webp 解码用
//! image crate（feature "webp" 已启用），首次用到才解码并缓存
//! （`RenderImage` 构造走 [`crate::image_upload::render_image`] 唯一入口）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui_kit::RenderImage;
use gpui_kit::component::input::{InlineTokenContext, InputContent};
use gpui_kit::prelude::*;

pub(crate) use crate::emoji_files::EMOJI_FILES;

/// 一段解析结果：纯文本或一张表情图。
pub(crate) enum EmojiSegment {
    Text(String),
    Emoji(Arc<RenderImage>),
}

/// 名字 → 解码后的 RenderImage 缓存（首次用到才解码；毒锁恢复为空表——
/// 解码失败重试代价只是一次 webp 解码）。名字先归一到静态表的 `'static`
/// 条目，缓存键才不借运行时字符串。表情选择器网格同源复用。
pub(crate) fn emoji_by_name(name: &str) -> Option<Arc<RenderImage>> {
    type EmojiCache = HashMap<&'static str, Option<Arc<RenderImage>>>;
    static CACHE: OnceLock<Mutex<EmojiCache>> = OnceLock::new();
    let static_name = EMOJI_FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(n, _)| *n)?;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(hit) = guard.get(static_name) {
        return hit.clone();
    }
    let decoded = EMOJI_FILES
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, bytes)| image::load_from_memory(bytes).ok())
        .map(image::DynamicImage::into_rgba8)
        .map(crate::image_upload::render_image)
        .map(Arc::new);
    guard.insert(static_name, decoded.clone());
    decoded
}

/// 文本 → 段序列：认识的 `[名字]` 换表情，不认识的原样保留为文本。
/// 手写扫描（不引 regex 依赖）：`[` 起始、1–8 个非空白字符且不含 `]`/`[`、
/// `]` 收尾（窗口判定在 [`crate::emoji_scan`]，与 Tauri 正则同口径），
/// 且整个代码在映射表里才认。
pub(crate) fn parse_emoji_segments(text: &str) -> Vec<EmojiSegment> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let Some(close) = crate::emoji_scan::find_emoji_code_end(text, i) else {
            i += 1;
            continue;
        };
        let name = &text[i..=close];
        let Some(img) = emoji_by_name(name) else {
            i += 1;
            continue;
        };
        if i > cursor {
            segments.push(EmojiSegment::Text(text[cursor..i].to_string()));
        }
        segments.push(EmojiSegment::Emoji(img));
        i = close + 1;
        cursor = i;
    }
    if cursor < text.len() {
        segments.push(EmojiSegment::Text(text[cursor..].to_string()));
    }
    segments
}

/// 段宽度估算：文本按 CJK/非 CJK 估宽，表情按 1.3 字号（hgplayer 同款）。
pub(crate) fn segment_widths(text: &str, font_px: f32) -> f32 {
    parse_emoji_segments(text)
        .iter()
        .map(|seg| match seg {
            EmojiSegment::Text(t) => {
                t.chars()
                    .map(|c| if is_wide_char(c) { 1.0 } else { 0.55 })
                    .sum::<f32>()
                    * font_px
            }
            EmojiSegment::Emoji(..) => font_px * 1.3,
        })
        .sum()
}

/// 宽字符判定（CJK/谚文/全形；估宽够用，不引 unicode-width 依赖——
/// gpui 依赖树里它与 unicode-properties 的版本钉子有过冲突史）。
pub(crate) fn is_wide_char(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x2FFFD
    )
}

// —— 输入框的内联 token（Tauri RichEmojiInput 对齐）——
//
// Tauri 的输入框是 contentEditable：`[名字]` 与 `@名字\u{FFFC}` 在输入框里
// 即时渲染成表情图/提及 chip，value 始终是含代码的纯文本。gpui-component 的
// Input 原生支持原子内联 token（`Input::token` 自定义渲染 + `replace_with_token`
// 插入，编辑/撤销/光标仍归输入框管）。附着逻辑的纯函数在
// [`crate::emoji_scan`]（可测性口径同文件头），这里只注入码表并负责渲染。

pub(crate) use crate::emoji_scan::emoji_inline_token;

/// 全文按输入框的 token 约定重解析：认识的 `[名字]` 附表情 token、
/// `@名字\u{FFFC}` 附提及 token。用于 `set_value` 的三个全量重置点
/// （提及插入 / 发送后清空）——点选插入走 `replace_with_token` 增量附着，
/// 不经这里。
pub(crate) fn rich_content(text: &str) -> InputContent {
    crate::emoji_scan::rich_content(text, |code| {
        EMOJI_FILES.iter().any(|(known, _)| *known == code)
    })
}

/// token 渲染器（`Input::token` 的回调体）：表情 token 画表情图、提及
/// token 画蓝色 @名字，选中态垫一层白 15% 底（与胶囊皮肤同源）。不认识的
/// 代码回退为原文案——兜的是「种子/历史文本里存了已下架表情」。
pub(crate) fn render_emoji_token(ctx: &InlineTokenContext) -> gpui_kit::AnyElement {
    let text = ctx.token().text();
    let mut el = gpui_kit::div()
        .flex()
        .items_center()
        .when(ctx.is_selected(), |el| {
            el.bg(gpui_kit::rgba(0xffffff26)).rounded(gpui_kit::px(4.0))
        });
    if let Some(bitmap) = emoji_by_name(text) {
        el = el.child(
            gpui_kit::img(gpui_kit::ImageSource::Render(bitmap))
                .size(ctx.line_height())
                .object_fit(gpui_kit::ObjectFit::Contain),
        );
    } else if let Some(name) = text.trim_end_matches('\u{FFFC}').strip_prefix('@') {
        el = el
            .mr_1()
            .text_color(gpui_kit::rgb(0x38bdf8))
            .child(format!("@{name}"));
    } else {
        el = el.child(text.to_string());
    }
    el.into_any_element()
}
