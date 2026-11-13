use super::model::{Action, Snapshot, Toggle};
use super::model::{Overrides, Target};

fn target(vid: &str) -> Target {
    Target {
        series_id: "series".into(),
        vid: vid.into(),
        title: "title".into(),
        likes: 0,
        collections: 0,
        comments: 0,
    }
}

#[test]
fn confirmed_collection_survives_episode_changes_but_never_crosses_accounts() {
    let mut overrides = Overrides::default();
    overrides.remember(
        "alice",
        &target("one"),
        Action::Collect,
        Toggle::new(true, 21),
    );
    overrides.remember("alice", &target("one"), Action::Like, Toggle::new(true, 5));
    let next = overrides.apply("alice", &target("two"), Snapshot::default());
    assert_eq!(next.collect, Toggle::new(true, 21));
    assert_eq!(next.like, Toggle::default());
    assert_eq!(
        overrides.apply("bob", &target("one"), Snapshot::default()),
        Snapshot::default()
    );
}

#[test]
fn liking_changes_the_count_immediately_and_failure_restores_it() {
    let mut state = Snapshot {
        like: Toggle::new(false, 7),
        ..Default::default()
    };
    let before = state.begin(Action::Like).unwrap();
    assert!(state.like.selected);
    assert!(state.like.pending);
    assert_eq!(state.like.count, 8);
    assert!(state.begin(Action::Like).is_none());
    state.finish(Action::Like, before, false);
    assert_eq!(state.like, Toggle::new(false, 7));
}

#[test]
fn unlike_never_produces_a_negative_count_and_success_keeps_the_selection() {
    let mut state = Snapshot {
        like: Toggle::new(true, 0),
        ..Default::default()
    };
    let before = state.begin(Action::Like).unwrap();
    assert!(!state.like.selected);
    assert_eq!(state.like.count, 0);
    state.finish(Action::Like, before, true);
    assert!(!state.like.pending);
    assert!(!state.like.selected);
}

#[test]
fn concurrent_actions_only_rollback_their_own_state() {
    let mut state = Snapshot::default();
    let before_like = state.begin(Action::Like).unwrap();
    let before_collect = state.begin(Action::Collect).unwrap();
    state.finish(Action::Like, before_like, false);
    state.finish(Action::Collect, before_collect, true);
    assert_eq!(state.like, Toggle::default());
    assert_eq!(state.collect, Toggle::new(true, 1));
}

#[test]
fn follow_completion_does_not_modify_public_like_or_collection_counts() {
    let mut state = Snapshot {
        like: Toggle::new(true, 12),
        collect: Toggle::new(false, 20),
        ..Default::default()
    };
    let before = state.begin(Action::Follow).unwrap();
    state.finish(Action::Follow, before, true);
    assert!(state.follow.selected);
    assert_eq!(state.like.count, 12);
    assert_eq!(state.collect.count, 20);
}
