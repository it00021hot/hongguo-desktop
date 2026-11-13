use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(1);
fn token() -> String {
    format!("read-test-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

struct Bytes {
    data: Vec<u8>,
    complete: bool,
    seen_limit: AtomicUsize,
}
impl ReadSource for Bytes {
    fn size(&self) -> Result<Option<u64>, ReadError> {
        Ok(self.complete.then_some(self.data.len() as u64))
    }
    fn read_at(
        &self,
        offset: u64,
        limit: usize,
        _: &ReadCancellation,
    ) -> Result<ReadResult, ReadError> {
        self.seen_limit.store(limit, Ordering::Relaxed);
        let at =
            usize::try_from(offset).map_err(|_| ReadError::Invalid("offset overflow".into()))?;
        if at >= self.data.len() {
            return Ok(ReadResult::Eof);
        }
        Ok(ReadResult::Data(
            self.data[at..(at + limit).min(self.data.len())].to_vec(),
        ))
    }
}
fn bytes(data: Vec<u8>, complete: bool) -> Arc<Bytes> {
    Arc::new(Bytes {
        data,
        complete,
        seen_limit: AtomicUsize::new(0),
    })
}

#[test]
fn bounds_partial_reads_and_eof_are_explicit() {
    let source = bytes(vec![7; MAX_READ_BYTES + 5], true);
    let registration = register_source(&token(), source.clone()).unwrap();
    let reader = open_source(registration.url()).unwrap();
    assert_eq!(
        reader.read_at(0, MAX_READ_BYTES * 2).unwrap(),
        ReadResult::Data(vec![7; MAX_READ_BYTES])
    );
    assert_eq!(source.seen_limit.load(Ordering::Relaxed), MAX_READ_BYTES);
    assert_eq!(
        reader.read_at(MAX_READ_BYTES as u64, 100).unwrap(),
        ReadResult::Data(vec![7; 5])
    );
    assert_eq!(
        reader.read_at((MAX_READ_BYTES + 5) as u64, 1).unwrap(),
        ReadResult::Eof
    );
    assert_eq!(
        reader.read_at((MAX_READ_BYTES + 20) as u64, 1).unwrap(),
        ReadResult::Eof
    );
    assert!(matches!(reader.read_at(0, 0), Err(ReadError::Invalid(_))));
    assert!(matches!(
        reader.read_at(u64::MAX, 2),
        Err(ReadError::Invalid(_))
    ));
}

#[test]
fn open_sources_cannot_report_eof_and_invalid_provider_results_are_rejected() {
    let registration = register_source(&token(), bytes(Vec::new(), false)).unwrap();
    let reader = open_source(registration.url()).unwrap();
    assert_eq!(reader.size().unwrap(), None);
    assert!(matches!(reader.read_at(0, 1), Err(ReadError::Invalid(_))));
    struct Invalid;
    impl ReadSource for Invalid {
        fn size(&self) -> Result<Option<u64>, ReadError> {
            Ok(Some(1))
        }
        fn read_at(&self, _: u64, _: usize, _: &ReadCancellation) -> Result<ReadResult, ReadError> {
            Ok(ReadResult::Data(Vec::new()))
        }
    }
    let registration = register_source(&token(), Arc::new(Invalid)).unwrap();
    assert!(matches!(
        open_source(registration.url()).unwrap().read_at(0, 1),
        Err(ReadError::Invalid(_))
    ));
}

#[test]
fn duplicate_reserved_malformed_and_revoked_tokens_never_fall_back() {
    let id = token();
    let source = bytes(vec![1], true);
    let registration = register_source(&id, source.clone()).unwrap();
    let url = registration.url().to_owned();
    assert!(matches!(
        register_source(&id, source),
        Err(ReadError::DuplicateToken)
    ));
    assert!(is_read_source_url(&url));
    assert!(is_read_source_url("HONGGUO-READ:malformed"));
    assert!(open_source("hongguo-read:malformed").is_err());
    assert!(open_source("hongguo-read://absent").is_err());
    assert!(!is_read_source_url("https://example.invalid/movie.mp4"));
    assert!(register_source("bad/../token", bytes(vec![1], true)).is_err());
    let old_reader = open_source(&url).unwrap();
    drop(registration);
    assert!(matches!(open_source(&url), Err(ReadError::UnknownSource)));
    assert!(matches!(old_reader.size(), Err(ReadError::Revoked)));
    let fresh = register_source(&id, bytes(vec![2], true)).unwrap();
    let fresh_reader = open_source(fresh.url()).unwrap();
    old_reader.close();
    assert_eq!(
        fresh_reader.read_at(0, 1).unwrap(),
        ReadResult::Data(vec![2])
    );
}

struct Waiting {
    entered: mpsc::Sender<()>,
}
impl ReadSource for Waiting {
    fn size(&self) -> Result<Option<u64>, ReadError> {
        Ok(None)
    }
    fn read_at(
        &self,
        _: u64,
        _: usize,
        cancel: &ReadCancellation,
    ) -> Result<ReadResult, ReadError> {
        self.entered.send(()).unwrap();
        while !cancel.is_cancelled() {
            std::thread::park_timeout(Duration::from_millis(1));
        }
        Err(ReadError::Cancelled)
    }
}

#[test]
fn reader_close_cancels_only_itself_and_registration_revokes_all_waiters() {
    let (entered, started) = mpsc::channel();
    let registration = register_source(&token(), Arc::new(Waiting { entered })).unwrap();
    let first = Arc::new(open_source(registration.url()).unwrap());
    let second = Arc::new(open_source(registration.url()).unwrap());
    let (ended, results) = mpsc::channel();
    let launch = |label, reader: Arc<ReadLease>| {
        let ended = ended.clone();
        std::thread::spawn(move || {
            ended.send((label, reader.read_at(0, 1))).unwrap();
        })
    };
    let first_work = launch("first", first.clone());
    let second_work = launch("second", second.clone());
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    first.close();
    assert_eq!(
        results.recv_timeout(Duration::from_secs(1)).unwrap(),
        ("first", Err(ReadError::Cancelled))
    );
    assert!(results.try_recv().is_err());
    assert_eq!(second.size().unwrap(), None);
    drop(registration);
    assert_eq!(
        results.recv_timeout(Duration::from_secs(1)).unwrap(),
        ("second", Err(ReadError::Revoked))
    );
    first_work.join().unwrap();
    second_work.join().unwrap();
}

#[test]
fn data_completed_after_close_is_not_delivered() {
    struct Delayed {
        entered: mpsc::Sender<()>,
        release: std::sync::Mutex<mpsc::Receiver<()>>,
    }
    impl ReadSource for Delayed {
        fn size(&self) -> Result<Option<u64>, ReadError> {
            Ok(Some(1))
        }
        fn read_at(&self, _: u64, _: usize, _: &ReadCancellation) -> Result<ReadResult, ReadError> {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(ReadResult::Data(vec![9]))
        }
    }
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let registration = register_source(
        &token(),
        Arc::new(Delayed {
            entered,
            release: std::sync::Mutex::new(wait),
        }),
    )
    .unwrap();
    let reader = Arc::new(open_source(registration.url()).unwrap());
    let running = reader.clone();
    let work = std::thread::spawn(move || running.read_at(0, 1));
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    reader.close();
    release.send(()).unwrap();
    assert_eq!(work.join().unwrap(), Err(ReadError::Cancelled));
}
