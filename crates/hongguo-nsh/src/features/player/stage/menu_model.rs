use std::ops::Range;

pub(super) const GROUP_SIZE: usize = 30;
pub(super) const RATES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

pub(super) fn group_for(episodes: &[u32], current: u32) -> usize {
    episodes
        .iter()
        .position(|episode| *episode == current)
        .unwrap_or(0)
        / GROUP_SIZE
}

pub(super) fn group_slice(len: usize, group: usize) -> Range<usize> {
    let last_group = len.saturating_sub(1) / GROUP_SIZE;
    let start = group.min(last_group) * GROUP_SIZE;
    start..(start + GROUP_SIZE).min(len)
}

pub(super) fn group_label(episodes: &[u32], group: usize) -> String {
    let slice = &episodes[group_slice(episodes.len(), group)];
    slice
        .first()
        .zip(slice.last())
        .map(|(first, last)| format!("{first}-{last}"))
        .unwrap_or_default()
}

pub(super) fn adjacent(episodes: &[u32], current: u32, offset: isize) -> Option<u32> {
    let at = episodes.iter().position(|episode| *episode == current)?;
    episodes.get(at.checked_add_signed(offset)?).copied()
}
