#[path = "../src/seek_gate.rs"]
mod seek_gate;
use seek_gate::SeekGate;

#[test]
fn seek_needs_real_audio_and_video_in_either_completion_order() {
    for audio_first in [false, true] {
        let mut gate = SeekGate::default();
        assert!(!gate.waiting());
        assert!(gate.accepts(0));
        assert!(!gate.complete());
        gate.begin(7);
        assert!(gate.waiting());
        assert!(gate.needs_video());
        if audio_first {
            gate.audio_completed();
        } else {
            gate.video_completed(7);
        }
        assert!(gate.waiting());
        assert!(!gate.complete());
        if audio_first {
            gate.video_completed(7);
        } else {
            gate.audio_completed();
        }
        assert!(!gate.waiting());
        assert!(gate.complete());
        assert!(!gate.needs_video());
    }
}

#[test]
fn replacing_a_seek_discards_the_previous_generations_gpu_completion() {
    let mut gate = SeekGate::default();
    gate.begin(1);
    gate.audio_completed();
    gate.begin(2);
    gate.video_completed(1);
    assert!(!gate.accepts(1));
    assert!(gate.accepts(2));
    assert!(gate.waiting());
    assert!(gate.needs_video());
    gate.audio_completed();
    assert!(!gate.complete());
    gate.video_completed(2);
    assert!(gate.complete());
}
