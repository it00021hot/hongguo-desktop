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
