use super::*;

struct Producer {
    path: PathBuf,
    session: Arc<Session>,
}
impl Producer {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hg-prefix-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("720.play.mp4");
        let session = begin(&path, true).unwrap();
        Self { path, session }
    }
    fn ready(&self) {
        std::fs::write(&self.path, b"first fragment").unwrap();
        hongguo_protocol::grow::commit(&self.path, 14);
        self.session.ready();
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.session.cancel();
        hongguo_protocol::grow::cancel(&self.path);
        clear(&self.path, &self.session);
        std::fs::remove_dir_all(self.path.parent().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn concurrent_native_reentry_reuses_one_producer_without_overwriting_its_file() {
    let producer = Producer::new();
    producer.ready();
    let (first, second) = tokio::join!(attach(&producer.path), attach(&producer.path));
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    assert_eq!(first.url, second.url);
    assert!(first.progressive && second.progressive);
    assert_eq!(std::fs::read(&producer.path).unwrap(), b"first fragment");
    assert!(Arc::ptr_eq(
        slots().get(&producer.path).unwrap(),
        &producer.session
    ));
}

#[tokio::test]
async fn reentry_before_the_first_commit_waits_for_that_producers_prefix() {
    let producer = Producer::new();
    let mut waiting = Box::pin(attach(&producer.path));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut waiting)
            .await
            .is_err()
    );
    producer.ready();
    assert!(waiting.await.unwrap().unwrap().progressive);
}

#[tokio::test]
async fn dropping_one_prefix_waiter_does_not_cancel_the_producer_or_other_waiters() {
    let producer = Producer::new();
    let mut abandoned = Box::pin(attach(&producer.path));
    let mut survivor = Box::pin(attach(&producer.path));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut abandoned)
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut survivor)
            .await
            .is_err()
    );
    drop(abandoned);
    assert!(!producer.session.cancelled());
    producer.ready();
    assert!(survivor.await.unwrap().unwrap().progressive);
}

#[tokio::test]
async fn failed_or_cancelled_sessions_never_publish_an_old_committed_prefix() {
    let producer = Producer::new();
    producer.ready();
    producer
        .session
        .settle(&Err::<(), _>(AppError::Media("decoder failed".into())));
    hongguo_protocol::grow::fail(&producer.path, "decoder failed");
    assert!(
        matches!(attach(&producer.path).await, Err(AppError::Media(message)) if message.contains("decoder failed"))
    );
    producer.session.ready();
    assert!(matches!(
        attach(&producer.path).await,
        Err(AppError::Media(_))
    ));
}

#[tokio::test]
async fn an_old_waiter_and_cleanup_cannot_join_or_remove_a_new_generation_at_the_same_path() {
    let producer = Producer::new();
    let mut old = Box::pin(attach(&producer.path));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut old)
            .await
            .is_err()
    );
    cancel(&producer.path);
    clear(&producer.path, &producer.session);
    let replacement = begin(&producer.path, true).unwrap();
    std::fs::write(&producer.path, b"new fragment").unwrap();
    hongguo_protocol::grow::commit(&producer.path, 12);
    replacement.ready();
    clear(&producer.path, &producer.session);
    assert!(matches!(old.await, Err(AppError::Cancelled)));
    assert!(attach(&producer.path).await.unwrap().unwrap().progressive);
    assert!(!replacement.cancelled());
    assert!(Arc::ptr_eq(
        slots().get(&producer.path).unwrap(),
        &replacement
    ));
    cancel(&producer.path);
    clear(&producer.path, &replacement);
}

#[tokio::test]
async fn done_requires_a_sealed_complete_file_and_foreign_grow_sessions_are_not_joined() {
    let producer = Producer::new();
    producer.ready();
    hongguo_protocol::grow::finish(&producer.path);
    producer.session.settle(&Ok(()));
    assert!(matches!(
        attach(&producer.path).await,
        Err(AppError::Media(_))
    ));
    cache::seal_complete(&producer.path).unwrap();
    assert!(!attach(&producer.path).await.unwrap().unwrap().progressive);
    clear(&producer.path, &producer.session);
    hongguo_protocol::grow::open(&producer.path).unwrap();
    hongguo_protocol::grow::commit(&producer.path, 14);
    assert!(attach(&producer.path).await.unwrap().is_none());
}

#[tokio::test]
async fn tauri_prefix_keeps_its_original_busy_behavior() {
    let producer = Producer::new();
    cancel(&producer.path);
    clear(&producer.path, &producer.session);
    let tauri = begin(&producer.path, false).unwrap();
    assert!(attach(&producer.path).await.unwrap().is_none());
    assert!(matches!(
        begin(&producer.path, false),
        Err(AppError::Busy(_))
    ));
    cancel(&producer.path);
    clear(&producer.path, &tauri);
}

#[tokio::test]
async fn a_busy_producer_finishing_before_attach_is_a_complete_cache_hit() {
    let producer = Producer::new();
    assert!(matches!(
        begin(&producer.path, true),
        Err(AppError::Busy(_))
    ));
    producer.ready();
    cache::seal_complete(&producer.path).unwrap();
    hongguo_protocol::grow::finish(&producer.path);
    producer.session.settle(&Ok(()));
    clear(&producer.path, &producer.session);
    let play = attach(&producer.path)
        .await
        .unwrap()
        .expect("completed producer remains reusable");
    assert!(play.cached);
    assert!(!play.progressive);
}

#[tokio::test]
async fn a_cancelled_output_with_a_seal_cannot_be_reused_by_the_native_fastpath() {
    let producer = Producer::new();
    producer.ready();
    cancel(&producer.path);
    cache::seal_complete(&producer.path).unwrap();
    clear(&producer.path, &producer.session);
    assert!(!cache::is_complete(&producer.path));
    assert!(attach(&producer.path).await.unwrap().is_none());
}

#[tokio::test]
async fn native_entry_attaches_before_plain_fetch_and_busy_race_joins_the_same_producer() {
    let mut producer = Producer::new();
    let root = producer.path.parent().unwrap().to_owned();
    let scope = hongguo_store::paths::ScopedDataDir::new(&root);
    cancel(&producer.path);
    clear(&producer.path, &producer.session);
    producer.path = cache::playback_file("native-join", 1, Some(720));
    std::fs::create_dir_all(producer.path.parent().unwrap()).unwrap();
    producer.session = begin(&producer.path, true).unwrap();
    producer.ready();
    let store = hongguo_store::Store::open_memory().unwrap();
    let state = Arc::new(crate::app_state::AppStateInner::with_db(store));
    let early =
        super::super::compat_for_native_playback_prefix(&state, "native-join", 1, None, Some(720))
            .await
            .unwrap();
    let raced = super::super::ensure_inner(
        "native-join",
        1,
        Some(720),
        super::super::CompatSource::LocalFile(root.join("must-not-be-read.mp4")),
        |_| {},
        |_| {},
        super::super::Delivery::NativePrefix,
    )
    .await
    .unwrap();
    assert_eq!(early.url, raced.url);
    assert!(early.progressive && raced.progressive);
    assert_eq!(std::fs::read(&producer.path).unwrap(), b"first fragment");
    drop(state);
    drop(producer);
    drop(scope);
    std::fs::remove_dir_all(root).unwrap();
}
