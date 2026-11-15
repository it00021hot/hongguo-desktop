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
