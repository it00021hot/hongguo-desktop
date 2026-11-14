use super::*;
use std::{
    collections::HashSet,
    sync::mpsc,
    time::{Duration, Instant},
};

const MOVIE: &[u8] = include_bytes!("../../tests/fixtures/hevc-main-136x76-aac.mp4");
const TEST_TIMEOUT: Duration = Duration::from_secs(3);

struct Memory {
    bytes: &'static [u8],
    threads: Mutex<HashSet<std::thread::ThreadId>>,
}
impl Memory {
    fn movie() -> Self {
        Self {
            bytes: MOVIE,
            threads: Mutex::default(),
        }
    }
}
impl ReadAt for Memory {
    fn length(&self) -> AppResult<u64> {
        Ok(self.bytes.len() as u64)
    }
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancelled: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        self.threads
            .lock()
            .unwrap()
            .insert(std::thread::current().id());
        if cancelled.load(Ordering::Acquire) {
            return Err(AppError::Cancelled);
        }
        let end = offset
            .checked_add(length)
            .ok_or_else(|| AppError::Media("test read overflow".into()))?;
        let data = self
            .bytes
            .get(offset as usize..end as usize)
            .ok_or_else(|| AppError::Media("test read beyond EOF".into()))?;
        bytes.clear();
        bytes.extend_from_slice(data);
        Ok(())
    }
}

fn wait<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        if let Some(result) = poll() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "session did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn picture(session: &DirectSession, clock: u64) -> Arc<PixelFrame> {
    wait(|| session.frame_at(clock).unwrap())
}

#[test]
fn paused_seek_presents_a_new_generation_before_the_audio_clock_can_advance() {
    let session = DirectSession::open(Arc::new(Memory::movie())).unwrap();
    assert_eq!(
        wait(|| session.info().unwrap()),
        FrameInfo {
            width: 136,
            height: 76
        }
    );
    let old = picture(&session, 0);
    assert!(session.holding());
    assert!(!session.presented(old.generation, old.serial + 1).unwrap());
    assert!(session.presented(old.generation, old.serial).unwrap());
    let generation = session.seek(1_001).unwrap();
    assert!(!session.presented(old.generation, old.serial).unwrap());
    let next = picture(&session, 0);
    assert_eq!(next.generation, generation);
    assert!(next.pts_ms >= 1_000);
    assert!(next.serial > old.serial);
    assert_eq!(next.stride, next.width * 4);
    assert_eq!(
        next.pixels.len(),
        next.stride as usize * next.height as usize
    );
    assert!(
        next.pixels
            .iter()
            .skip(3)
            .step_by(4)
            .all(|alpha| *alpha == 255)
    );
    assert!(Arc::ptr_eq(
        &next,
        &session.frame_at(u64::MAX).unwrap().unwrap()
    ));
    assert!(session.presented(generation, next.serial).unwrap());
    assert!(!session.presented(generation, next.serial).unwrap());
    assert!(!session.holding());
}

#[test]
fn queue_backpressure_and_a_stopped_clock_preserve_the_displayed_frame() {
    let session = DirectSession::open(Arc::new(Memory::movie())).unwrap();
    wait(|| (session.shared.lock().queue.len() == QUEUE_DEPTH).then_some(()));
    let first = picture(&session, 0);
    session.presented(first.generation, first.serial).unwrap();
    wait(|| (session.shared.lock().queue.len() == QUEUE_DEPTH).then_some(()));
    for _ in 0..10 {
        assert!(Arc::ptr_eq(&first, &session.frame_at(0).unwrap().unwrap()));
        assert!(session.shared.lock().queue.len() <= QUEUE_DEPTH);
    }
    let later = picture(&session, 150);
    assert!(later.pts_ms > first.pts_ms && later.pts_ms <= 158);
}

struct BlockFirst {
    memory: Memory,
    first: AtomicBool,
    entered: mpsc::SyncSender<()>,
    cancelled: mpsc::SyncSender<()>,
    range: Option<(u64, u64)>,
}
impl ReadAt for BlockFirst {
    fn length(&self) -> AppResult<u64> {
        self.memory.length()
    }
    fn read_at(
        &self,
        offset: u64,
        length: u64,
        cancel: &AtomicBool,
        bytes: &mut Vec<u8>,
    ) -> AppResult<()> {
        self.memory
            .threads
            .lock()
            .unwrap()
            .insert(std::thread::current().id());
        if self
            .range
            .is_none_or(|(start, end)| offset >= start && offset < end)
            && self.first.swap(false, Ordering::AcqRel)
        {
            self.entered.send(()).unwrap();
            while !cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.cancelled.send(()).unwrap();
            return Err(AppError::Cancelled);
        }
        self.memory.read_at(offset, length, cancel, bytes)
    }
}
type Blocked = (Arc<BlockFirst>, mpsc::Receiver<()>, mpsc::Receiver<()>);
fn blocked() -> Blocked {
    blocked_in(None)
}
fn blocked_in(range: Option<(u64, u64)>) -> Blocked {
    let (entered, receive_entered) = mpsc::sync_channel(1);
    let (cancelled, receive_cancelled) = mpsc::sync_channel(1);
    (
        Arc::new(BlockFirst {
            memory: Memory::movie(),
            first: AtomicBool::new(true),
            entered,
            cancelled,
            range,
        }),
        receive_entered,
        receive_cancelled,
    )
}

#[test]
fn latest_seek_cancels_a_blocked_index_read_without_spawning_more_workers() {
    let (source, entered, cancelled) = blocked();
    let session = DirectSession::open(source.clone()).unwrap();
    entered.recv_timeout(TEST_TIMEOUT).unwrap();
    session.seek(500).unwrap();
    let generation = session.seek(1_500).unwrap();
    cancelled.recv_timeout(TEST_TIMEOUT).unwrap();
    let frame = picture(&session, 0);
    assert_eq!(frame.generation, generation);
    assert!(frame.pts_ms >= 1_500);
    assert_eq!(source.memory.threads.lock().unwrap().len(), 1);
}

#[test]
fn seeking_cancels_an_inflight_sample_read_and_reuses_the_prepared_index() {
    let mdat = hongguo_video::r#box::find_box(MOVIE, 0, MOVIE.len(), "mdat").unwrap();
    let (source, entered, cancelled) =
        blocked_in(Some((mdat.start as u64, (mdat.start + mdat.size) as u64)));
    let session = DirectSession::open(source.clone()).unwrap();
    entered.recv_timeout(TEST_TIMEOUT).unwrap();
    assert!(session.info().unwrap().is_some());
    let generation = session.seek(1_250).unwrap();
    cancelled.recv_timeout(TEST_TIMEOUT).unwrap();
    let frame = picture(&session, 0);
    assert_eq!(frame.generation, generation);
    assert!(frame.pts_ms >= 1_250);
    assert_eq!(source.memory.threads.lock().unwrap().len(), 1);
}

#[test]
fn decoding_to_eof_keeps_the_displayed_frame_and_can_seek_again() {
    let source = Arc::new(Memory::movie());
    let session = DirectSession::open(source.clone()).unwrap();
    let first = picture(&session, 0);
    session.presented(first.generation, first.serial).unwrap();
    let last = wait(|| {
        session
            .frame_at(u64::MAX)
            .unwrap()
            .filter(|frame| frame.pts_ms >= 1_958)
    });
    assert!(Arc::ptr_eq(
        &last,
        &session.frame_at(u64::MAX).unwrap().unwrap()
    ));
    assert!(session.info().unwrap().is_some());
    let generation = session.seek(0).unwrap();
    let restarted = picture(&session, 0);
    assert_eq!(restarted.generation, generation);
    assert_eq!(restarted.pts_ms, 0);
    assert_eq!(source.threads.lock().unwrap().len(), 1);
}

#[test]
fn close_wakes_the_pending_read_and_rejects_later_controls() {
    let (source, entered, cancelled) = blocked();
    let session = Arc::new(DirectSession::open(source).unwrap());
    entered.recv_timeout(TEST_TIMEOUT).unwrap();
    session.close();
    cancelled.recv_timeout(TEST_TIMEOUT).unwrap();
    assert!(session.info().is_err());
    assert!(session.frame_at(0).is_err());
    assert!(session.seek(0).is_err());
    assert!(session.presented(1, 1).is_err());
}

#[test]
fn decoder_errors_are_reported_to_frames_and_seeks() {
    let source = Arc::new(Memory {
        bytes: b"not an mp4",
        threads: Mutex::default(),
    });
    let session = DirectSession::open(source).unwrap();
    wait(|| session.info().err());
    assert!(session.frame_at(0).is_err());
    assert!(session.seek(0).is_err());
}
