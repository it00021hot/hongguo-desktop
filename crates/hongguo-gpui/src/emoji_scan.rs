//! 表情代码 `[名字]` 的边界扫描（纯函数，无外部依赖）。
//!
//! 独立成文件是为了可测性：本 crate 是 bin（无 lib 目标，`#[cfg(test)]`
//! 单测跑不了），集成测试经 `#[path = "../src/emoji_scan.rs"]` 引入本文件
//! （preview/scenes.rs 同款），所以这里**禁止 `crate::` 引用**。

/// 从 `text[open]`（调用方保证是 `[` 且在字符边界上）起找表情代码收尾
/// `]` 的字节下标。
///
/// 口径对齐 Tauri 壳 `danmaku-emoji.ts` 的 `/\[[^\][\s]{1,8}\]/`：`[` 后
/// 1–8 个**字符**（不是字节——中文名每字 3 字节，按字节限窗会把 3/4 字
/// 中文名漏掉）、非空白且不含 `[`/`]`；扫描窗 9 字符 = 8 个名字字符 +
/// 1 个可能的收尾 `]`。
pub(crate) fn find_emoji_code_end(text: &str, open: usize) -> Option<usize> {
    let body = &text[open + '['.len_utf8()..];
    for (k, ch) in body.char_indices().take(9) {
        match ch {
            ']' => return Some(open + 1 + k),
            '[' => return None,
            c if c.is_whitespace() => return None,
            _ => {}
        }
    }
    None
}

/// 一段提及拆分结果：纯文本或一个 @ 名字（渲染方决定染蓝/加后缀）。
pub(crate) enum MentionPiece {
    Text(String),
    Mention(String),
}

/// 文本中段的提及段拆分（Tauri EmojiText 的 `/@[^\s\u{FFFC}]+\u{FFFC}/g`
/// 同口径，名字可含 `@`）：`@名字\u{FFFC}` 记为提及段，其余为文本。
pub(crate) fn split_mention_segments(text: &str) -> Vec<MentionPiece> {
    let mut pieces = Vec::new();
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        // i 恒在字符边界（初值 0；步进要么按字符长度、要么落在 FFFC 之后）
        let Some(ch) = text[i..].chars().next() else {
            break;
        };
        if ch != '@' {
            i += ch.len_utf8();
            continue;
        }
        let mut end = None;
        for (k, c) in text[i + 1..].char_indices() {
            if c == '\u{FFFC}' {
                end = Some(k);
                break;
            }
            if c.is_whitespace() {
                break;
            }
        }
        let Some(k) = end else {
            i += ch.len_utf8();
            continue;
        };
        if k == 0 {
            i += ch.len_utf8();
            continue;
        }
        if i > cursor {
            pieces.push(MentionPiece::Text(text[cursor..i].to_string()));
        }
        pieces.push(MentionPiece::Mention(text[i + 1..i + 1 + k].to_string()));
        i = i + 1 + k + '\u{FFFC}'.len_utf8();
        cursor = i;
    }
    if cursor < text.len() {
        pieces.push(MentionPiece::Text(text[cursor..].to_string()));
    }
    pieces
}

// —— 输入框内联 token（`Input::token` 机制的附着逻辑）——
//
// Tauri 的输入框是 contentEditable：`[名字]` 与 `@名字\u{FFFC}` 即时渲染成
// 表情图/提及 chip，value 始终是含代码的纯文本。gpui-component 的 Input
// 原生支持原子内联 token，这里负责「一段纯文本该附着哪些 token」的纯逻辑；
// 渲染在 [`crate::danmaku_emoji`]（要在引入测试时保持自洽，本文件禁止
// `crate::` 引用，已知码表经 `is_known` 闭包注入）。

use gpui_kit::component::input::{InlineToken, InputContent};

/// 表情码（带括号的 `[名字]`，即码表键形态）的内联 token
/// （id/text/label 同为代码本身）。
pub(crate) fn emoji_inline_token(code: &str) -> InlineToken {
    InlineToken::new(code.to_string(), code.to_string())
}

/// 提及段 `@名字\u{FFFC}` 的内联 token（尾随空格不进 token，与 Tauri
/// serialize 同口径）。
fn mention_inline_token(text: &str) -> InlineToken {
    let name = text
        .trim_start_matches('@')
        .trim_end_matches('\u{FFFC}')
        .to_string();
    InlineToken::new(text.to_string(), text.to_string()).with_label(format!("@{name}"))
}

/// 全文按输入框的 token 约定重解析：认识的 `[名字]`（`is_known` 判定）
/// 附表情 token、`@名字\u{FFFC}` 附提及 token。用于 `set_value` 的全量
/// 重置点（提及插入 / 发送后清空）；点选插入走 `replace_with_token`
/// 增量附着，不经这里。
pub(crate) fn rich_content(text: &str, is_known: impl Fn(&str) -> bool) -> InputContent {
    let mut content = InputContent::new(text);
    // 表情码：与 [`find_emoji_code_end`] 同一把窗口扫描
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(close) = find_emoji_code_end(text, i)
        {
            // 扫出的 code 含括号，正是码表键形态，直接交给判定闭包
            let code = &text[i..=close];
            if is_known(code)
                && let Ok(next) = content
                    .clone()
                    .with_token(i..close + 1, emoji_inline_token(code))
            {
                content = next;
            }
            i = close + 1;
            continue;
        }
        i += 1;
    }
    // 提及：与 [`split_mention_segments`] 同口径——从首个可行 `@` 起贪心
    // （名字可含 `@`、到 FFFC 前无空白、名字非空），到 FFFC 收尾。切片
    // 必须按字符边界步进：FFFC 占 3 字节，`idx + 1` 不是边界（本轮
    // 「@小果闪退」的根因）。
    for (idx, c) in text.char_indices() {
        if c != '\u{FFFC}' {
            continue;
        }
        let Some(at) = text[..idx]
            .match_indices('@')
            .find(|(at, _)| at + 1 < idx && !text[at + 1..idx].chars().any(char::is_whitespace))
            .map(|(at, _)| at)
        else {
            continue;
        };
        let end = idx + c.len_utf8();
        if let Ok(next) = content
            .clone()
            .with_token(at..end, mention_inline_token(&text[at..end]))
        {
            content = next;
        }
    }
    content
}
