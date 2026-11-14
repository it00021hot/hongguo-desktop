#[path = "../src/seek_gate.rs"]
mod seek_gate;
use seek_gate::SeekGate;

#[test]
fn native_seek_waits_for_the_shared_sessions_visible_generation() {
    let mut gate = SeekGate::default();
    assert!(gate.accepts(0));
    assert!(!gate.take_complete());
    gate.begin(7);
    assert!(!gate.accepts(6));
    assert!(gate.accepts(7));
    assert!(!gate.take_complete());
    gate.presented(7);
    assert!(gate.take_complete());
    assert!(!gate.take_complete());
    assert!(gate.accepts(8));
}

#[test]
fn replacing_a_seek_discards_previous_generations_gpu_completion() {
    let mut gate = SeekGate::default();
    gate.begin(1);
    gate.begin(2);
    gate.presented(1);
    assert!(!gate.accepts(1));
    assert!(!gate.take_complete());
    gate.presented(2);
    assert!(gate.take_complete());
}

#[test]
fn device_recreation_releases_the_old_generation_filter() {
    let mut gate = SeekGate::default();
    gate.begin(1);
    gate.reset();
    gate.presented(1);
    assert!(!gate.take_complete());
    assert!(gate.accepts(2));
}

#[test]
fn an_empty_edit_completes_the_seek_without_a_nonexistent_media_frame() {
    let mut gate = SeekGate::default();
    gate.empty_edit();
    assert!(!gate.take_complete());
    gate.begin(9);
    gate.empty_edit();
    assert!(gate.take_complete());
    assert!(!gate.take_complete());
    gate.presented(8);
    assert!(!gate.take_complete());
    assert!(gate.accepts(10));
}
