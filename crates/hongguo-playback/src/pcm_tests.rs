use super::*;

fn ring() -> PcmRing {
    PcmRing::new(
        PcmFormat {
            sample_rate: 4,
            channels: 1,
        },
        16,
        1,
        10.0,
    )
    .unwrap()
}

fn chunk(start: f64, duration: f64, frames: usize) -> PcmChunk {
    PcmChunk {
        samples: vec![0.5; frames],
        media_start_secs: start,
        media_duration_secs: duration,
    }
}

#[test]
fn actual_device_consumption_drives_media_and_underflow_holds_position() {
    let ring = ring();
    ring.push(1, chunk(10.0, 1.0, 4), &AtomicBool::new(false))
        .unwrap();
    let mut output = [0.0; 8];
    assert_eq!(ring.read(&mut output).media_frames, 4);
    assert_eq!(ring.position(), 10.0);
    ring.played(1, 2);
    assert_eq!(ring.position(), 10.5);
    ring.played(1, 8);
    assert_eq!(ring.position(), 11.0);
    assert_eq!(&output[4..], &[0.0; 4]);
    ring.read(&mut output);
    ring.played(1, 16);
    assert_eq!(ring.position(), 11.0);
}

#[test]
fn queued_speed_segments_keep_their_original_mapping() {
    let ring = ring();
    let cancelled = AtomicBool::new(false);
    ring.push(1, chunk(10.0, 1.0, 4), &cancelled).unwrap();
    ring.push(1, chunk(11.0, 2.0, 4), &cancelled).unwrap();
    ring.read(&mut [0.0; 8]);
    ring.played(1, 6);
    assert_eq!(ring.position(), 12.0);
    ring.played(1, 8);
    assert_eq!(ring.position(), 13.0);
}

#[test]
fn reset_rejects_old_pcm_and_late_device_positions() {
    let ring = ring();
    ring.push(1, chunk(10.0, 1.0, 4), &AtomicBool::new(false))
        .unwrap();
    ring.reset(2, 30.0);
    ring.played(1, 4);
    assert_eq!(ring.position(), 30.0);
    assert!(
        ring.push(1, chunk(10.0, 1.0, 4), &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(ring.read(&mut [1.0; 4]).media_frames, 0);
}

#[test]
fn capacity_and_format_errors_are_explicit() {
    let ring = ring();
    assert!(
        ring.push(1, chunk(0.0, 1.0, 17), &AtomicBool::new(false))
            .is_err()
    );
    assert!(
        ring.push(1, chunk(0.0, 0.0, 4), &AtomicBool::new(false))
            .is_err()
    );
    ring.close();
    assert!(
        ring.push(1, chunk(0.0, 1.0, 4), &AtomicBool::new(false))
            .is_err()
    );
}

#[test]
fn realtime_callbacks_never_wait_for_the_producer_lock() {
    let ring = std::sync::Arc::new(ring());
    let locked = ring.state.lock();
    let (sender, receiver) = std::sync::mpsc::channel();
    let source = ring.clone();
    let callback = std::thread::spawn(move || {
        let mut output = [1.0; 4];
        let result = source.read(&mut output);
        source.played(result.epoch, 4);
        sender.send(output).unwrap();
    });
    let result = receiver.recv_timeout(std::time::Duration::from_millis(100));
    drop(locked);
    callback.join().unwrap();
    assert_eq!(
        result.expect("audio callback waited for the producer lock"),
        [0.0; 4]
    );
    ring.push(1, chunk(10.0, 1.0, 4), &AtomicBool::new(false))
        .unwrap();
    ring.read(&mut [0.0; 4]);
    ring.played(1, 6);
    assert_eq!(ring.position(), 10.5);
}

#[test]
fn a_control_query_preserves_the_final_position_after_a_callback_lock_miss() {
    let ring = ring();
    ring.push(1, chunk(10.0, 1.0, 4), &AtomicBool::new(false))
        .unwrap();
    ring.read(&mut [0.0; 4]);
    let producer = ring.state.lock();
    ring.played(1, 2);
    drop(producer);
    assert_eq!(ring.position(), 10.0);
    ring.synchronize_position(1, 2);
    assert_eq!(ring.position(), 10.5);
    ring.reset(2, 30.0);
    ring.synchronize_position(1, 4);
    assert_eq!(ring.position(), 30.0);
}
