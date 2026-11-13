use super::*;

#[test]
fn snapshot_waiters_receive_commit_terminal_and_cancellation_without_reading_media() {
    let path = source();
    let other = path.clone();
    let reader = thread::spawn(move || wait_for_change(&other, 6, || false).unwrap());
    std::fs::write(&path, b"prefix-tail").unwrap();
    commit(&path, 11);
    assert_eq!(
        reader.join().unwrap(),
        GrowthSnapshot {
            committed_bytes: 11,
            state: GrowthState::Open
        }
    );
    let other = path.clone();
    let reader = thread::spawn(move || wait_for_change(&other, 11, || false).unwrap());
    fail(&path, "failed after prefix");
    assert_eq!(
        reader.join().unwrap().state,
        GrowthState::Failed("failed after prefix".into())
    );
    assert!(wait_for_change(&path, 11, || true).is_err());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

fn source() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "hongguo-grow-reader-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("clip.mp4");
    std::fs::write(&path, b"prefix").unwrap();
    open(&path).unwrap();
    commit(&path, 6);
    path
}

#[test]
fn snapshots_distinguish_committed_bytes_from_every_terminal_state() {
    let path = source();
    assert_eq!(
        snapshot(&path),
        Some(GrowthSnapshot {
            committed_bytes: 6,
            state: GrowthState::Open
        })
    );
    fail(&path, "encoder failed");
    assert_eq!(
        snapshot(&path).unwrap().state,
        GrowthState::Failed("encoder failed".into())
    );
    let other = source();
    cancel(&other);
    assert_eq!(snapshot(&other).unwrap().state, GrowthState::Cancelled);
    let done = source();
    finish(&done);
    assert_eq!(
        snapshot(&done).unwrap(),
        GrowthSnapshot {
            committed_bytes: 6,
            state: GrowthState::Done
        }
    );
    assert_eq!(snapshot(&done.with_file_name("unregistered.mp4")), None);
    for path in [path, other, done] {
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn a_reader_receives_committed_bytes_before_the_file_finishes() {
    let path = source();
    assert_eq!(completed_len(&path), None);
    assert_eq!(read_cancellable(&path, 0, 20, || false).unwrap(), b"prefix");
    let (ready, started) = mpsc::channel();
    let (output, received) = mpsc::channel();
    let reader_path = path.clone();
    let reader = thread::spawn(move || {
        ready.send(()).unwrap();
        output
            .send(read_cancellable(&reader_path, 6, 20, || false))
            .unwrap();
    });
    started.recv().unwrap();
    std::fs::write(&path, b"prefix-tail").unwrap();
    commit(&path, 11);
    assert_eq!(
        received
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        b"-tail"
    );
    reader.join().unwrap();
    finish(&path);
    assert_eq!(completed_len(&path), Some(11));
    assert!(
        read_cancellable(&path, 11, 20, || false)
            .unwrap()
            .is_empty()
    );
    assert!(read_cancellable(&path, 12, 20, || false).is_err());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn cancellation_wakes_a_reader_without_waiting_for_another_commit() {
    let path = source();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let (ready, started) = mpsc::channel();
    let (output, received) = mpsc::channel();
    let reader_path = path.clone();
    let reader = thread::spawn(move || {
        ready.send(()).unwrap();
        output
            .send(read_cancellable(&reader_path, 6, 20, || {
                flag.load(Ordering::Acquire)
            }))
            .unwrap();
    });
    started.recv().unwrap();
    cancelled.store(true, Ordering::Release);
    assert!(
        received
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_err()
    );
    reader.join().unwrap();
    cancel(&path);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn failure_and_an_empty_completed_file_keep_their_terminal_semantics() {
    let failed = source();
    fail(&failed, "encoding failed");
    assert_eq!(
        read_cancellable(&failed, 6, 20, || false).unwrap_err(),
        "encoding failed"
    );
    let empty = source();
    std::fs::write(&empty, []).unwrap();
    finish(&empty);
    assert_eq!(completed_len(&empty), Some(0));
    assert!(
        read_cancellable(&empty, 0, 20, || false)
            .unwrap()
            .is_empty()
    );
    assert!(read_cancellable(&empty, 0, 0, || false).unwrap().is_empty());
    for path in [failed, empty] {
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
