//! 剧终接力的「下一季」挑选（Tauri `src/features/player/season.ts` 的
//! Rust 对应）。
//!
//! 数据面是 plan/v 的相关作品列表（`RelatedItem.works`）：**同系列各季带
//! 「第N季」角标，同 IP 作品不带**。服务端顺序不可信（实测《逆江湖》各季
//! 乱序），接力选季一律走季数：大于当前季的**最小已上线季**（episode_cnt
//! 0 = 未上线/预约，不可播，不当候选）。

use hongguo_api::detail::RelatedItem;

/// 「第N季」角标 → 季号（Tauri `seasonNoFromTag` 的 `/^第(\d{1,2})季$/`
/// 同口径：整串必须恰为「第N季」，`第3季全`/`第100季` 都判 None）。
pub(crate) fn season_no_from_tag(tag: &str) -> Option<u32> {
    let rest = tag.strip_prefix("第")?;
    let no: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if no.is_empty() || no.len() > 2 || &rest[no.len()..] != "季" {
        return None;
    }
    no.parse().ok()
}

/// 中文数字 → 阿拉伯（一~九十九够用：季数不会上百）。
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
    let ten_idx = s.find('十');
    match ten_idx {
        None => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            digit(c)
        }
        Some(i) => {
            let tens = if i == 0 {
                1
            } else {
                let prefix: String = s.chars().take_while(|c| *c != '十').collect();
                let mut chars = prefix.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                digit(c)?
            };
            let ones = if i == s.len() - '十'.len_utf8() {
                0
            } else {
                let suffix: String = s.chars().skip_while(|c| *c != '十').skip(1).collect();
                let mut chars = suffix.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                digit(c)?
            };
            Some(tens * 10 + ones)
        }
    }
}

/// 从标题里解析季数：「第三季」「第3季」都认，没有季标返回 None。
pub(crate) fn season_from_title(title: &str) -> Option<u32> {
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
        if !digits.is_empty() {
            let tail = trimmed[digits.len()..].trim_start();
            if tail.starts_with('季') {
                return chinese_numeral(&digits);
            }
        }
        rest = after;
    }
    None
}

/// 挑下一季：同系列（带季角标）里**大于当前季的最小已上线季**。
///
/// - 当前季数来源：works 里的自身条目角标 → 当前标题解析。都没有（无标
///   第一季，plan/v 又只回其他季）→ 视作第 1 季，接最小季。
/// - 返回 None = 判不出（列表无带标季 / 当前季比所有带标季都高），调用方
///   走自己的回落（首页信息流的下一条推荐）。
pub(crate) fn pick_next_season(
    works: &[RelatedItem],
    current_series_id: &str,
    current_title: &str,
) -> Option<RelatedItem> {
    let mut tagged: Vec<(u32, &RelatedItem)> = works
        .iter()
        .filter_map(|w| {
            season_no_from_tag(&w.tag)
                .filter(|_| w.episode_cnt > 0)
                .map(|no| (no, w))
        })
        .collect();
    if tagged.is_empty() {
        return None;
    }
    tagged.sort_by_key(|(no, _)| *no);
    let current_no = works
        .iter()
        .find(|w| w.series_id == current_series_id)
        .and_then(|w| season_no_from_tag(&w.tag))
        .or_else(|| season_from_title(current_title));
    match current_no {
        // 无标第一季：plan/v 只回其他季，剩下的带标季全是它的续季
        None => Some(tagged[0].1.clone()),
        Some(cur) => tagged
            .iter()
            .find(|(no, _)| *no > cur)
            .map(|(_, w)| (*w).clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(sid: &str, title: &str, tag: &str, ep: u32) -> RelatedItem {
        RelatedItem {
            series_id: sid.into(),
            title: title.into(),
            tag: tag.into(),
            episode_cnt: ep,
            ..Default::default()
        }
    }

    #[test]
    fn tag_and_title_parsing() {
        assert_eq!(season_no_from_tag("第3季"), Some(3));
        assert_eq!(season_no_from_tag("第3季全"), None);
        assert_eq!(season_no_from_tag("同IP"), None);
        assert_eq!(season_from_title("命运契约 第三季"), Some(3));
        assert_eq!(season_from_title("命运契约第 12 季"), Some(12));
        assert_eq!(season_from_title("命运契约 第二十季"), Some(20));
        // 没有「第」不算季标（Tauri 正则同款要求）
        assert_eq!(season_from_title("命运契约 二十季"), None);
        assert_eq!(season_from_title("命运契约"), None);
    }

    #[test]
    fn picks_smallest_online_season_above_current() {
        // 乱序 + 未上线季：只认带标且可播的，挑大于当前季的最小季
        let works = vec![
            item("s3", "第三季", "第3季", 30),
            item("s2", "第二季", "第2季", 0), // 未上线，不可选
            item("s4", "第四季", "第4季", 40),
            item("ip", "同IP作品", "", 20),
        ];
        let cur = item("s1", "命运契约", "", 24);
        // 当前季是 works 里的自身条目（无标，视作第 1 季）→ 接第 3 季
        let mut all = works.clone();
        all.push(cur.clone());
        let next = pick_next_season(&all, "s1", &cur.title).expect("应挑到下一季");
        assert_eq!(next.series_id, "s3");
        // 当前季不在列表：标题解析第 2 季 → 接第 3 季
        let next = pick_next_season(&works, "sx", "某某 第二季").expect("应挑到下一季");
        assert_eq!(next.series_id, "s3");
        // 当前季比所有带标季都高 → None
        assert!(pick_next_season(&works, "s5", "某某 第五季").is_none());
    }
}
