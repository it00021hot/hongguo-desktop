#[derive(Debug, PartialEq)]
pub(super) struct DefinitionChange {
    pub definition: Option<u32>,
    pub resume_at: f64,
    pub reset_definition: bool,
}

/// Start preparing the next native source this far from the end.
pub(super) const HANDOFF_STAGE_REMAINING_SECS: f64 = 8.0;
/// Freeze the outgoing picture this far from the end, before native End paints black.
/// Wider than the shown progress interval so a tick cannot skip the window.
pub(super) const HANDOFF_HOLD_REMAINING_SECS: f64 = 0.5;
pub(super) const INCOMING_PROGRESS_INTERVAL_MS: f32 = 50.0;

/// 「加载中」只在没有可播画面、且 resource 尚无 Playback 时出现。
pub(super) fn cold_status_loading(has_surface: bool, has_playback: bool) -> bool {
    !has_surface && !has_playback
}

pub(super) fn incoming_picture_ready(loaded: bool, seeking: bool, progressed: bool) -> bool {
    loaded && !seeking && progressed
}

pub(super) fn can_promote_incoming(
    incoming_url: &str,
    resource_url: Option<&str>,
    picture_ready: bool,
) -> bool {
    picture_ready && resource_url == Some(incoming_url)
}

pub(super) fn can_finish_handoff(
    incoming_url: &str,
    resource_url: Option<&str>,
    picture_ready: bool,
    faded: bool,
) -> bool {
    faded && can_promote_incoming(incoming_url, resource_url, picture_ready)
}

pub(super) fn incoming_opacity(revealing: bool) -> f32 {
    if revealing { 1.0 } else { 0.0 }
}

pub(super) fn should_stage_next(remaining: f64, auto_next: bool, already_staged: bool) -> bool {
    auto_next
        && !already_staged
        && remaining.is_finite()
        && remaining > HANDOFF_HOLD_REMAINING_SECS
        && remaining <= HANDOFF_STAGE_REMAINING_SECS
}

pub(super) fn should_hold_outgoing(remaining: f64, duration: f64, auto_next: bool) -> bool {
    auto_next
        && duration.is_finite()
        && duration > HANDOFF_HOLD_REMAINING_SECS
        && remaining.is_finite()
        && (0.0..=HANDOFF_HOLD_REMAINING_SECS).contains(&remaining)
}

pub(super) fn reuse_staged(staged: Option<(&str, u32)>, series: &str, episode: u32) -> bool {
    staged == Some((series, episode))
}

pub(super) fn incoming_candidate<'a>(
    shown_url: Option<&str>,
    staged_url: Option<&'a str>,
    resource_url: Option<&'a str>,
) -> Option<&'a str> {
    staged_url
        .filter(|url| shown_url != Some(*url))
        .or_else(|| resource_url.filter(|url| shown_url != Some(*url)))
}

pub(super) fn outgoing_paused(
    handoff: bool,
    play_allowed: bool,
    current: bool,
    user_paused: bool,
) -> bool {
    handoff || !play_allowed || !current || user_paused
}

pub(super) fn incoming_paused(picture_ready: bool) -> bool {
    picture_ready
}

pub(super) fn drop_shown_without_resource(loading: bool, handoff: bool) -> bool {
    !loading && !handoff
}

pub(super) fn change_definition(
    current: Option<u32>,
    requested: Option<u32>,
    at: f64,
) -> Option<DefinitionChange> {
    (current != requested).then_some(DefinitionChange {
        definition: requested,
        reset_definition: requested.is_none(),
        resume_at: if at.is_finite() && at >= 0.0 { at } else { 0.0 },
    })
}

pub(super) fn change_episode(current: u32, requested: u32) -> bool {
    current != requested
}

pub(super) fn change_source(current: &(String, u32), requested: &(String, u32)) -> bool {
    current != requested
}

pub(super) fn initial_seek(resume_at: f64) -> Option<f64> {
    (resume_at.is_finite() && resume_at > 0.0).then_some(resume_at)
}

pub(super) fn source_current(loading: bool, failed: bool, same_lease: bool) -> bool {
    !loading && !failed && same_lease
}

pub(super) fn seek_available(target: f64, native_duration: f64) -> bool {
    target.is_finite()
        && target >= 0.0
        && (!native_duration.is_finite() || native_duration <= 0.0 || target < native_duration)
}

#[derive(Default)]
pub(super) struct SeekLatch {
    issued: Option<f64>,
}

impl SeekLatch {
    pub fn dispatch(&mut self, target: f64) -> bool {
        if self.issued == Some(target) {
            return false;
        }
        self.issued = Some(target);
        true
    }

    pub fn confirmed(&self, target: f64, actual: f64) -> bool {
        const NATIVE_CONFIRMATION_TOLERANCE: f64 = 1.0;
        self.issued == Some(target)
            && actual.is_finite()
            && actual >= 0.0
            && (actual - target).abs() <= NATIVE_CONFIRMATION_TOLERANCE
    }

    pub fn clear(&mut self) {
        self.issued = None;
    }
}

#[derive(Default)]
pub(super) struct LoadLatch {
    loaded: bool,
}

impl LoadLatch {
    pub fn seek(&mut self, pending: Option<f64>, saved: f64, duration: f64) -> Option<f64> {
        if self.loaded {
            return None;
        }
        self.loaded = true;
        let at = pending.unwrap_or(saved);
        Some(
            if at.is_finite()
                && at >= 0.0
                && (!duration.is_finite() || duration <= 0.0 || at < duration)
            {
                at
            } else {
                0.0
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_status_loading_only_without_surface_or_playback() {
        assert!(cold_status_loading(false, false));
        assert!(!cold_status_loading(true, false));
        assert!(!cold_status_loading(false, true));
        assert!(!cold_status_loading(true, true));
    }

    #[test]
    fn incoming_picture_ready_needs_load_progress_and_no_seek() {
        assert!(!incoming_picture_ready(true, false, false));
        assert!(!incoming_picture_ready(true, true, true));
        assert!(!incoming_picture_ready(false, false, true));
        assert!(incoming_picture_ready(true, false, true));
    }

    #[test]
    fn can_promote_incoming_only_when_resource_owns_ready_url() {
        assert!(!can_promote_incoming("next", Some("next"), false));
        assert!(!can_promote_incoming("next", Some("current"), true));
        assert!(!can_promote_incoming("next", None, true));
        assert!(can_promote_incoming("next", Some("next"), true));
    }

    #[test]
    fn handoff_keeps_outgoing_until_the_incoming_fade_finishes() {
        assert!(!can_finish_handoff("next", Some("next"), true, false));
        assert!(!can_finish_handoff("next", Some("next"), false, true));
        assert!(can_finish_handoff("next", Some("next"), true, true));
    }

    #[test]
    fn incoming_stays_transparent_until_the_crossfade_starts() {
        assert_eq!(incoming_opacity(false), 0.0);
        assert_eq!(incoming_opacity(true), 1.0);
    }

    #[test]
    fn should_stage_next_in_the_overlap_window() {
        assert!(!should_stage_next(15.0, true, false));
        assert!(should_stage_next(8.0, true, false));
        assert!(should_stage_next(1.0, true, false));
        assert!(!should_stage_next(0.5, true, false));
        assert!(!should_stage_next(4.0, false, false));
        assert!(!should_stage_next(4.0, true, true));
    }

    #[test]
    fn should_hold_outgoing_near_end_once_the_title_has_length() {
        assert!(!should_hold_outgoing(0.5, 0.1, true));
        assert!(should_hold_outgoing(0.5, 60.0, true));
        assert!(should_hold_outgoing(0.0, 60.0, true));
        assert!(!should_hold_outgoing(1.0, 60.0, true));
        assert!(!should_hold_outgoing(0.1, 60.0, false));
    }

    #[test]
    fn reuse_staged_only_for_the_matching_episode() {
        assert!(reuse_staged(Some(("s", 2)), "s", 2));
        assert!(!reuse_staged(Some(("s", 2)), "s", 3));
        assert!(!reuse_staged(None, "s", 2));
    }

    #[test]
    fn change_source_treats_series_or_episode_as_a_new_handoff() {
        assert!(change_source(&("a".into(), 1), &("b".into(), 1)));
        assert!(change_source(&("a".into(), 1), &("a".into(), 2)));
        assert!(!change_source(&("a".into(), 1), &("a".into(), 1)));
    }

    #[test]
    fn incoming_candidate_keeps_shown_and_prefers_staged() {
        assert_eq!(
            incoming_candidate(Some("a"), Some("b"), Some("c")),
            Some("b")
        );
        assert_eq!(
            incoming_candidate(Some("a"), Some("a"), Some("c")),
            Some("c")
        );
        assert_eq!(incoming_candidate(Some("a"), Some("a"), Some("a")), None);
        assert_eq!(incoming_candidate(None, None, Some("a")), Some("a"));
    }

    #[test]
    fn outgoing_stays_frozen_during_handoff() {
        assert!(outgoing_paused(true, true, true, false));
        assert!(!outgoing_paused(false, true, true, false));
        assert!(outgoing_paused(false, true, true, true));
    }

    #[test]
    fn incoming_holds_after_first_picture() {
        assert!(!incoming_paused(false));
        assert!(incoming_paused(true));
    }

    #[test]
    fn drop_shown_only_when_resource_is_gone_and_not_handing_off() {
        assert!(!drop_shown_without_resource(true, false));
        assert!(!drop_shown_without_resource(false, true));
        assert!(drop_shown_without_resource(false, false));
    }
}
