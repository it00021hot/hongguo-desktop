//! 表情代码扫描器与提及段拆分（`emoji_scan.rs` 经 `#[path]` 引入——
//! bin crate 无 lib 目标，这是唯一能把 crate 内纯函数接进测试的通路）。

#[path = "../src/emoji_scan.rs"]
mod emoji_scan;

use emoji_scan::{MentionPiece, find_emoji_code_end, split_mention_segments};

/// 以文本中第一个 `[` 为起点找收尾 `]`。
fn scan(text: &str) -> Option<usize> {
    let open = text.find('[')?;
    find_emoji_code_end(text, open)
}

/// 3/4 字中文名必须收尾——这是字节窗 bug（10/53 码永远不识别）的直接
/// 回归样本，53 表全量的中长名都在这里。
#[test]
fn chinese_long_names_close_within_char_window() {
    for code in [
        "[做鬼脸]",
        "[翻白眼]",
        "[快哭了]",
        "[你细品]",
        "[赶稿中]",
        "[学会了]",
        "[求爆更]",
        "[求关注]",
        "[没看够]",
        "[重拳出击]",
    ] {
        let text = format!("看{code}之后");
        let close = scan(&text).unwrap_or_else(|| panic!("{code} 应被认出收尾"));
        assert_eq!(&text[text.find('[').unwrap()..=close], code);
    }
}

/// 短名与 ASCII 名不受影响（老口径回归）。
#[test]
fn short_names_still_close() {
    let text = "来了[微笑]好";
    let close = scan(text).expect("[微笑] 应收尾");
    assert_eq!(&text[text.find('[').unwrap()..=close], "[微笑]");
}

/// 满 8 字符的名字收尾，第 9 个字符起不认（hgplayer 1–8 约束）。
#[test]
fn eight_char_name_closes_nine_does_not() {
    let text = "[一二三四五六七八]";
    assert!(scan(text).is_some(), "8 字名应收尾");
    let text = "[一二三四五六七八九]";
    assert!(scan(text).is_none(), "9 字名不应收尾");
}

/// 空白（含换行）/嵌套 `[`/未闭合都失败——与 Tauri 正则 `\s` 同口径。
#[test]
fn boundaries_and_unterminated_fail() {
    for text in ["[好 点]", "[好\n点]", "[[好]", "[好"] {
        assert!(scan(text).is_none(), "{text:?} 不应收尾");
    }
}

/// 起点不在首字符：从指定 `[` 起、跨字符正确计窗。
#[test]
fn scan_from_later_open() {
    let text = "a[微笑]";
    let open = text.find('[').unwrap();
    assert_eq!(
        find_emoji_code_end(text, open),
        Some(open + "[微笑]".len() - 1)
    );
}

/// 提及拆分的段序列简述（"t"=文本，"m"=提及名）。
fn outline(text: &str) -> Vec<(&'static str, String)> {
    split_mention_segments(text)
        .into_iter()
        .map(|p| match p {
            MentionPiece::Text(t) => ("t", t),
            MentionPiece::Mention(n) => ("m", n),
        })
        .collect()
}

/// 中段提及拆分：`@名字\u{FFFC}` 成提及段、其余留文本。首个 `@` 到
/// FFFC 之间无空白时整体算一个名字——贪心首匹配与 Tauri 的
/// `/@[^\s\uFFFC]+\uFFFC/g` 逐字同口径（两壳渲染一致就是对齐目标）。
/// 字节步进版曾在这里 panic 在多字节字符中间（preview 矩阵抓到的真 bug）。
#[test]
fn mention_segments_split_mid_text() {
    assert_eq!(
        outline("回复 @小果：这剧@张三\u{FFFC}真好[微笑]"),
        vec![
            ("t", "回复 ".to_string()),
            ("m", "小果：这剧@张三".to_string()),
            ("t", "真好[微笑]".to_string()),
        ]
    );
    // 多个提及并存：各归各段（@ 前的空格留在文本段，与正则一致）
    assert_eq!(
        outline("看@张三\u{FFFC}的剧评 @李四\u{FFFC} 好"),
        vec![
            ("t", "看".to_string()),
            ("m", "张三".to_string()),
            ("t", "的剧评 ".to_string()),
            ("m", "李四".to_string()),
            ("t", " 好".to_string()),
        ]
    );
}

/// 无提及的普通中文文本原样成单段；空名（`@\uFFFC`）不算提及。
#[test]
fn mention_segments_plain_text_stays_whole() {
    assert_eq!(
        outline("这剧真好[微笑]"),
        vec![("t", "这剧真好[微笑]".to_string())]
    );
    assert_eq!(outline("@\u{FFFC}"), vec![("t", "@\u{FFFC}".to_string())]);
}

// —— 输入框内联 token 的附着逻辑（rich_content）——
//
// 码表注入为两码假表；「已知」判定与壳的 EMOJI_FILES 对表同构。
fn tokens(text: &str) -> Vec<(usize, usize, String, String)> {
    use emoji_scan::rich_content;
    let content = rich_content(text, |code| {
        ["[微笑]", "[笑]"].contains(&code)
    });
    content
        .tokens()
        .iter()
        .map(|s| {
            (
                s.range().start,
                s.range().end,
                s.token().text().to_string(),
                s.token().label().to_string(),
            )
        })
        .collect()
}

/// @提及插入必 panic 的字节边界回归：FFFC 占 3 字节，`idx + 1` 切在
/// 字符中间（2026-10-23「@小果闪退」的根因——提及插入走 rich_content，
/// 一点候选就 abort）。
#[test]
fn mention_token_range_ends_on_char_boundary() {
    let t = "@小果AI\u{FFFC} 早上好";
    let toks = tokens(t);
    assert_eq!(toks.len(), 1, "恰好一个提及 token：{toks:?}");
    let (start, end, text, label) = &toks[0];
    assert_eq!(*start, 0);
    assert_eq!(*end, t.find('\u{FFFC}').unwrap() + '\u{FFFC}'.len_utf8());
    assert_eq!(text, "@小果AI\u{FFFC}");
    assert_eq!(label, "@小果AI");
}

/// 表情码与提及混排：各自附着、互不重叠；提及从首个可行 `@` 贪心
/// （名字可含 `@`，与 split_mention_segments / Tauri 正则同口径）。
#[test]
fn emoji_and_mention_tokens_attach_disjoint() {
    let t = "哈[微笑]@小果AI\u{FFFC}好";
    let toks = tokens(t);
    assert_eq!(toks.len(), 2, "{toks:?}");
    assert_eq!((toks[0].0, toks[0].2.clone()), (t.find('[').unwrap(), "[微笑]".to_string()));
    assert!(toks[1].2.starts_with("@小果AI"));
}

/// 贪心首匹配：首个 `@` 到 FFFC 之间无空白时整体算一个名字。
#[test]
fn mention_token_greedy_first_at() {
    let t = "回复 @小果：这剧@张三\u{FFFC}真好";
    let toks = tokens(t);
    assert_eq!(toks.len(), 1, "{toks:?}");
    assert_eq!(toks[0].2, "@小果：这剧@张三\u{FFFC}");
    assert_eq!(toks[0].3, "@小果：这剧@张三");
}

/// 空名（`@\uFFFC`）与跨空白都是非提及；未知码不附着。
#[test]
fn non_mentions_and_unknown_codes_stay_plain() {
    assert!(tokens("@\u{FFFC}").is_empty());
    assert!(tokens("看 @小果 \u{FFFC} 好").is_empty(), "FFFC 前有空白");
    assert!(tokens("[不存在的码]").is_empty());
}
