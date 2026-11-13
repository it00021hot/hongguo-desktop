//! 同系列接季的纯领域规则。调用方保留原始作品，规则只返回其位置，
//! 不把 API 传输类型或 UI 状态引进公共领域模型。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeasonCandidate<'a> {
    pub id: &'a str,
    pub tag: &'a str,
    pub episode_count: u32,
}

/// 与 Tauri `/^第(\d{1,2})季$/` 保持同一严格角标契约。
pub fn season_no_from_tag(tag: &str) -> Option<u32> {
    let rest = tag.strip_prefix('第')?;
    let no: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if no.is_empty() || no.len() > 2 || &rest[no.len()..] != "季" {
        return None;
    }
    no.parse().ok()
}

fn chinese_numeral(s: &str) -> Option<u32> {
    if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
        return s.parse().ok();
    }
    let digit = |c: char| match c {
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    };
    match s.find('十') {
        None => {
            let mut chars = s.chars();
            let value = digit(chars.next()?)?;
            chars.next().is_none().then_some(value)
        }
        Some(i) => {
            let tens = if i == 0 {
                1
            } else {
                let mut prefix = s[..i].chars();
                let value = digit(prefix.next()?)?;
                if prefix.next().is_some() {
                    return None;
                }
                value
            };
            let suffix = &s[i + '十'.len_utf8()..];
            let ones = if suffix.is_empty() {
                0
            } else {
                let mut chars = suffix.chars();
                let value = digit(chars.next()?)?;
                if chars.next().is_some() {
                    return None;
                }
                value
            };
            Some(tens * 10 + ones)
        }
    }
}

/// 标题里的「第三季」「第 12 季」，不把没有「第」的普通标题当季数。
pub fn season_from_title(title: &str) -> Option<u32> {
    let mut rest = title;
    while let Some(pos) = rest.find('第') {
        let after = &rest[pos + '第'.len_utf8()..];
        let trimmed = after.trim_start();
        let digits: String = trimmed
            .chars()
            .take_while(|c| {
                c.is_ascii_digit()
                    || matches!(
                        c,
                        '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '两'
                    )
            })
            .collect();
        if !digits.is_empty() && trimmed[digits.len()..].trim_start().starts_with('季') {
            return chinese_numeral(&digits);
        }
        rest = after;
    }
    None
}

/// 挑大于当前季的最小已上线季，忽略服务端排列与同 IP 无季标作品。
/// 当前条目角标优先于标题；均没有时继承现有无标第一季规则。
pub fn next_season_index(
    works: &[SeasonCandidate<'_>],
    current_id: &str,
    current_title: &str,
) -> Option<usize> {
    let current_no = works
        .iter()
        .find(|work| work.id == current_id)
        .and_then(|work| season_no_from_tag(work.tag))
        .or_else(|| season_from_title(current_title));
    works
        .iter()
        .enumerate()
        .filter_map(|(at, work)| {
            let no = season_no_from_tag(work.tag)?;
            (work.episode_count > 0 && current_no.is_none_or(|current| no > current))
                .then_some((no, at))
        })
        .min_by_key(|(no, _)| *no)
        .map(|(_, at)| at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item<'a>(id: &'a str, tag: &'a str, count: u32) -> SeasonCandidate<'a> {
        SeasonCandidate {
            id,
            tag,
            episode_count: count,
        }
    }

    #[test]
    fn strict_tags_and_title_fallback_preserve_the_existing_gpui_contract() {
        assert_eq!(season_no_from_tag("第3季"), Some(3));
        for tag in ["第3季全", "同IP", "第100季", "第三季"] {
            assert_eq!(season_no_from_tag(tag), None);
        }
        assert_eq!(season_from_title("命运契约 第三季"), Some(3));
        assert_eq!(season_from_title("命运契约第 12 季"), Some(12));
        assert_eq!(season_from_title("命运契约 第二十季"), Some(20));
        assert_eq!(season_from_title("命运契约 二十季"), None);
        assert_eq!(season_from_title("命运契约"), None);
    }

    #[test]
    fn chooses_the_smallest_online_season_in_server_order_independent_inputs() {
        let works = [
            item("s3", "第3季", 30),
            item("s2", "第2季", 0),
            item("s4", "第4季", 40),
            item("ip", "", 20),
            item("s1", "", 24),
        ];
        assert_eq!(next_season_index(&works, "s1", "命运契约"), Some(0));
        assert_eq!(next_season_index(&works, "sx", "某某 第二季"), Some(0));
        assert_eq!(next_season_index(&works, "s5", "某某 第五季"), None);
    }

    #[test]
    fn own_tag_wins_over_title_and_a_known_last_season_does_not_loop() {
        let works = [
            item("now", "第3季", 10),
            item("two", "第2季", 10),
            item("four", "第4季", 10),
        ];
        assert_eq!(next_season_index(&works, "now", "标题第一季"), Some(2));
        assert_eq!(next_season_index(&works, "four", "标题第一季"), None);
    }
}
