#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SystemEmoji {
    pub code: &'static str,
    pub uri: &'static str,
}
include!(concat!(env!("OUT_DIR"), "/emoji_catalog.rs"));
impl std::fmt::Debug for SystemEmoji {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Emoji").field(&self.code).finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Segment {
    Text(String),
    Emoji(&'static SystemEmoji),
    Mention { raw: String, name: String },
}

pub(crate) fn parse(text: &str) -> Vec<Segment> {
    let mut parts = Vec::new();
    let mut cursor = 0;
    let mut at = 0;
    while at < text.len() {
        let current = text[at..].chars().next().expect("character boundary");
        let known = (current == '[')
            .then(|| code_end(text, at))
            .flatten()
            .and_then(|end| {
                SYSTEM
                    .iter()
                    .find(|item| item.code == &text[at..end])
                    .map(|item| (end, item))
            });
        if let Some((end, emoji)) = known {
            mentions(&text[cursor..at], &mut parts);
            parts.push(Segment::Emoji(emoji));
            cursor = end;
            at = end;
        } else {
            at += current.len_utf8();
        }
    }
    mentions(&text[cursor..], &mut parts);
    parts
}

fn code_end(text: &str, at: usize) -> Option<usize> {
    for (count, (offset, ch)) in text[at + 1..].char_indices().take(9).enumerate() {
        match ch {
            ']' if count > 0 => return Some(at + offset + 2),
            '[' | ']' => return None,
            ch if ch.is_whitespace() => return None,
            _ => {}
        }
    }
    None
}

fn mentions(text: &str, parts: &mut Vec<Segment>) {
    let mut cursor = 0;
    let mut at = 0;
    while at < text.len() {
        let current = text[at..].chars().next().expect("character boundary");
        if current == '@'
            && let Some((end, _)) = text[at + 1..]
                .char_indices()
                .take_while(|(_, ch)| !ch.is_whitespace())
                .find(|(_, ch)| *ch == '\u{fffc}')
            && end > 0
        {
            if cursor < at {
                parts.push(Segment::Text(text[cursor..at].into()));
            }
            let name_end = at + 1 + end;
            let end = name_end + '\u{fffc}'.len_utf8();
            parts.push(Segment::Mention {
                raw: text[at..end].into(),
                name: text[at + 1..name_end].into(),
            });
            at = end;
            cursor = end;
        } else {
            at += current.len_utf8();
        }
    }
    if cursor < text.len() {
        parts.push(Segment::Text(text[cursor..].into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_system_codes_render_inline_but_unknown_or_malformed_codes_remain_text() {
        let smile = SYSTEM.iter().find(|item| item.code == "[微笑]").unwrap();
        assert_eq!(
            parse("你好[微笑][未知][x y]"),
            vec![
                Segment::Text("你好".into()),
                Segment::Emoji(smile),
                Segment::Text("[未知][x y]".into())
            ]
        );
        assert!(parse("").is_empty());
        assert_eq!(
            parse("[[][微笑]"),
            vec![Segment::Text("[[]".into()), Segment::Emoji(smile)]
        );
    }

    #[test]
    fn emoji_and_mentions_keep_a_lossless_text_wire_representation() {
        let input = "😀@小果AI\u{fffc} [爱慕]@普通名字\u{fffc} 未结束@foo";
        let parts = parse(input);
        assert!(
            parts
                .iter()
                .any(|part| matches!(part, Segment::Mention { name, .. } if name == "小果AI"))
        );
        let roundtrip: String = parts
            .into_iter()
            .map(|part| match part {
                Segment::Text(text) | Segment::Mention { raw: text, .. } => text,
                Segment::Emoji(emoji) => emoji.code.to_owned(),
            })
            .collect();
        assert_eq!(roundtrip, input);
    }
}
