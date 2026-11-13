use std::collections::BTreeSet;

pub(crate) fn parse_range(input: &str, indices: &[u32]) -> Vec<u32> {
    let available = indices.iter().copied().collect::<BTreeSet<_>>();
    let mut selected = BTreeSet::new();
    for part in input.split([',', '，']).map(str::trim) {
        let bounds = if let Some((from, to)) = part.split_once('-') {
            positive(from).zip(positive(to))
        } else {
            positive(part).map(|value| (value, value))
        };
        if let Some((from, to)) = bounds
            && from <= to
        {
            selected.extend(available.range(from..=to).copied());
        }
    }
    selected.into_iter().collect()
}

fn positive(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|value| *value > 0)
}

pub(crate) fn format_range(indices: &[u32]) -> String {
    let sorted = indices.iter().copied().collect::<BTreeSet<_>>();
    let mut sorted = sorted.into_iter();
    let Some(mut start) = sorted.next() else {
        return String::new();
    };
    let mut previous = start;
    let mut parts = Vec::new();
    for value in sorted {
        if previous.checked_add(1) == Some(value) {
            previous = value;
            continue;
        }
        parts.push(interval(start, previous));
        start = value;
        previous = value;
    }
    parts.push(interval(start, previous));
    parts.join(", ")
}

fn interval(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Preset {
    All,
    Invert,
    Clear,
    FirstTen,
    FirstThirty,
    LastThirty,
}

pub(crate) fn preset(mode: Preset, indices: &[u32], selected: &BTreeSet<u32>) -> BTreeSet<u32> {
    let available = indices.iter().copied().collect::<BTreeSet<_>>();
    match mode {
        Preset::All => available,
        Preset::Clear => BTreeSet::new(),
        Preset::Invert => available.difference(selected).copied().collect(),
        Preset::FirstTen => available.into_iter().take(10).collect(),
        Preset::FirstThirty => available.into_iter().take(30).collect(),
        Preset::LastThirty => available.into_iter().rev().take(30).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_select_only_real_ids_and_bound_huge_intervals_without_expansion() {
        let ids = [1, 3, 4, 7, 9];
        assert_eq!(parse_range("1-4,7，8-100", &ids), ids);
        assert_eq!(parse_range("1-4294967295", &ids), ids);
        assert_eq!(parse_range("4,4, 1, bad, +3, 9-2", &ids), vec![1, 4]);
        assert!(parse_range("", &ids).is_empty());
        assert!(parse_range("0, +3, 9-2, nothing", &ids).is_empty());
        assert!(parse_range("1-4", &[]).is_empty());
    }
    #[test]
    fn formatting_sorts_deduplicates_and_compacts_runs() {
        assert_eq!(format_range(&[9, 3, 4, 4, 1]), "1, 3-4, 9");
        assert_eq!(format_range(&[2]), "2");
        assert_eq!(format_range(&[]), "");
    }
    #[test]
    fn presets_use_real_episode_ids_instead_of_invented_indices() {
        let ids = [3, 9, 12];
        let selected = BTreeSet::from([9]);
        assert_eq!(preset(Preset::All, &ids, &selected), BTreeSet::from(ids));
        assert_eq!(
            preset(Preset::Invert, &ids, &selected),
            BTreeSet::from([3, 12])
        );
        assert!(preset(Preset::Clear, &ids, &selected).is_empty());
        assert_eq!(
            preset(Preset::FirstTen, &ids, &selected),
            BTreeSet::from(ids)
        );
        assert_eq!(
            preset(Preset::FirstThirty, &ids, &selected),
            BTreeSet::from(ids)
        );
        assert_eq!(
            preset(Preset::LastThirty, &ids, &selected),
            BTreeSet::from(ids)
        );
    }
}
