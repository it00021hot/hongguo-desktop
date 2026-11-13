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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_episode_numbers_follow_their_position_without_inventing_missing_episodes() {
        let episodes = (1..=80).map(|index| index * 3).collect::<Vec<_>>();
        assert_eq!(group_for(&episodes, 93), 1);
        assert_eq!(group_for(&episodes, 240), 2);
        assert_eq!(group_for(&episodes, 2), 0);
    }

    #[test]
    fn ranges_clamp_after_a_series_changes_and_allow_empty_series() {
        assert_eq!(group_slice(80, 1), 30..60);
        assert_eq!(group_slice(80, 2), 60..80);
        assert_eq!(group_slice(3, 50), 0..3);
        assert_eq!(group_slice(0, 50), 0..0);
        assert_eq!(group_label(&[], 0), "");
        assert_eq!(group_label(&[3, 6, 9], 2), "3-9");
    }

    #[test]
    fn keyboard_steps_use_available_identities_and_stop_at_grid_boundaries() {
        let episodes = (1..=80).map(|index| index * 3).collect::<Vec<_>>();
        assert_eq!(adjacent(&episodes, 93, 1), Some(96));
        assert_eq!(adjacent(&episodes, 3, 8), Some(27));
        assert_eq!(adjacent(&episodes, 3, -1), None);
        assert_eq!(adjacent(&episodes, 240, 1), None);
        assert_eq!(adjacent(&episodes, 1, 1), None);
    }
}
