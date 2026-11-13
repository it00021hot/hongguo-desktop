use super::super::{MediaServer, source::Source};
use hongguo_nsh_video_api::read_source::{self, ReadError, ReadResult};
use hongguo_protocol::grow::{self, GrowthState};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

struct GrowingFile(std::path::PathBuf);
impl GrowingFile {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("hg-native-reader-{}.mp4", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"prefix").unwrap();
        grow::open(&path).unwrap();
        grow::commit(&path, 6);
        Self(path)
    }
}
impl Drop for GrowingFile {
    fn drop(&mut self) {
        grow::cancel(&self.0);
        std::fs::remove_file(&self.0).unwrap();
    }
}

#[tokio::test]
async fn native_reader_waits_for_commit_and_reader_close_preserves_shared_producer() {
    let file = GrowingFile::new();
    let server = MediaServer::start().await.unwrap();
    let lease = server
        .register_native(Source::file(&file.0).unwrap())
        .unwrap();
    let first = Arc::new(read_source::open_source(lease.url()).unwrap());
    let second = read_source::open_source(lease.url()).unwrap();
    assert_eq!(first.size().unwrap(), None);
    assert_eq!(
        first.read_at(0, 64).unwrap(),
        ReadResult::Data(b"prefix".to_vec())
    );
    let (ended, result) = mpsc::channel();
    let running = first.clone();
    let worker = std::thread::spawn(move || ended.send(running.read_at(6, 10)).unwrap());
    assert!(result.recv_timeout(Duration::from_millis(120)).is_err());
    first.close();
    assert_eq!(
        result.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(ReadError::Cancelled)
    );
    worker.join().unwrap();
    assert_eq!(grow::snapshot(&file.0).unwrap().state, GrowthState::Open);
    assert_eq!(
        second.read_at(0, 1).unwrap(),
        ReadResult::Data(b"p".to_vec())
    );
    let (ended, result) = mpsc::channel();
    let worker = std::thread::spawn(move || ended.send(second.read_at(6, 10)).unwrap());
    assert!(result.recv_timeout(Duration::from_millis(120)).is_err());
    std::fs::write(&file.0, b"prefix-tail").unwrap();
    grow::commit(&file.0, 11);
    assert_eq!(
        result
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        ReadResult::Data(b"-tail".to_vec())
    );
    worker.join().unwrap();
}

#[tokio::test]
async fn native_lease_revokes_blocked_readers_without_cancelling_other_lease() {
    let file = GrowingFile::new();
    let server = MediaServer::start().await.unwrap();
    let lease = server
        .register_native(Source::file(&file.0).unwrap())
        .unwrap();
    let other = server
        .register_native(Source::file(&file.0).unwrap())
        .unwrap();
    let reader = read_source::open_source(lease.url()).unwrap();
    let other_reader = read_source::open_source(other.url()).unwrap();
    let (ended, result) = mpsc::channel();
    let worker = std::thread::spawn(move || ended.send(reader.read_at(6, 10)).unwrap());
    assert!(result.recv_timeout(Duration::from_millis(120)).is_err());
    drop(lease);
    assert_eq!(
        result.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(ReadError::Revoked)
    );
    worker.join().unwrap();
    assert_eq!(grow::snapshot(&file.0).unwrap().state, GrowthState::Open);
    assert_eq!(
        other_reader.read_at(0, 1).unwrap(),
        ReadResult::Data(b"p".to_vec())
    );
    grow::fail(&file.0, "producer test failure");
    assert_eq!(
        other_reader.read_at(0, 1),
        Err(ReadError::Io("producer test failure".into()))
    );
    assert!(other_reader.size().is_err());
}

#[tokio::test]
async fn fixed_native_file_has_partial_reads_and_real_eof() {
    let file = GrowingFile::new();
    grow::finish(&file.0);
    let server = MediaServer::start().await.unwrap();
    let lease = server
        .register_native(Source::file(&file.0).unwrap())
        .unwrap();
    let reader = read_source::open_source(lease.url()).unwrap();
    assert_eq!(reader.size().unwrap(), Some(6));
    assert_eq!(
        reader.read_at(4, 100).unwrap(),
        ReadResult::Data(b"ix".to_vec())
    );
    assert_eq!(reader.read_at(6, 100).unwrap(), ReadResult::Eof);
    assert_eq!(reader.read_at(100, 100).unwrap(), ReadResult::Eof);
    drop(server);
    assert!(reader.size().is_err());
}
