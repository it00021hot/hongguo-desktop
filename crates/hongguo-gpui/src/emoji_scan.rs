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
