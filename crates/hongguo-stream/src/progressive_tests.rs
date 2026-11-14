use super::*;
use crate::{StreamCache, read_range, read_range_cancellable, total_len};
use std::sync::atomic::AtomicBool;

fn ready_stream(vid: &str, definition: u32, len: usize) {
    let cache = crate::cache();
    cache.remove(vid, definition);
    cache.store(
        vid,
        definition,
        &(0..len).map(|i| (i % 251) as u8).collect::<Vec<_>>(),
    );
}

fn stream(
    sparse: Arc<SparseBuffer>,
    plan: Option<Arc<StreamingPlan>>,
    plain_len: u64,
) -> Arc<ProgressiveStream> {
    Arc::new(ProgressiveStream {
        sparse,
        plan,
        plain_len,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    })
}

fn register(vid: &str, progressive: Arc<ProgressiveStream>) {
    let cache = crate::cache();
    cache.remove(vid, 720);
    assert!(cache.begin_fetch(vid, 720));
    cache.end_fetch(vid, 720);
    assert!(cache.set_progressive(vid, 720, progressive));
}

fn progressive_fixture(vid: &str) -> (Arc<SparseBuffer>, Vec<u8>, Vec<u8>, u64) {
    use hongguo_video::{decrypt_buffer::decrypt_mp4_buffer, streaming_tests::assemble};
    let (_, cipher, key) = assemble(true);
    let reference = decrypt_mp4_buffer(&cipher, &key).unwrap();
    let len = cipher.len() as u64;
    let (offset, size) = hongguo_video::streaming::locate_moov(&cipher, 0, len).unwrap();
    let plan = StreamingPlan::build(
        &cipher[offset as usize..(offset + size) as usize],
        offset,
        len,
        &key,
    )
    .unwrap();
    let plain_len = plan.plain_len();
    let sparse = SparseBuffer::new(len);
    register(vid, stream(sparse.clone(), Some(Arc::new(plan)), plain_len));
    (sparse, cipher, reference, plain_len)
}

#[test]
fn whole_mode_reads_ranges_clamps_at_eof_and_rejects_out_of_bounds() {
    ready_stream("bytes-whole", 1080, 4096);
    let chunk = read_range("bytes-whole", 1080, 100, Some(50)).unwrap();
    assert_eq!(chunk.total, 4096);
    assert_eq!(chunk.start, 100);
    assert_eq!(
        chunk.bytes,
        (100..150).map(|i| (i % 251) as u8).collect::<Vec<_>>()
    );
    assert_eq!(
        read_range("bytes-whole", 1080, 4090, Some(100))
            .unwrap()
            .bytes
            .len(),
        6
    );
    assert!(
        read_range("bytes-whole", 1080, 4096, None)
            .unwrap()
            .bytes
            .is_empty()
    );
    assert!(
        read_range("bytes-whole", 1080, 4090, Some(0))
            .unwrap()
            .bytes
            .is_empty()
    );
    assert!(matches!(
        read_range("bytes-whole", 1080, 4097, Some(1)),
        Err(StreamReadError::OutOfRange { .. })
    ));
}

#[test]
fn progressive_reads_decrypt_after_data_arrives_with_known_length() {
    let (sparse, cipher, reference, len) = progressive_fixture("bytes-encrypted");
    assert_eq!(total_len("bytes-encrypted", 720), Some(len));
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        sparse.write(0, &cipher);
    });
    let chunk = read_range("bytes-encrypted", 720, 10, Some(20)).unwrap();
    writer.join().unwrap();
    assert_eq!(chunk.bytes, reference[10..30]);
    assert_eq!(chunk.total, len);
}

#[test]
fn cancelling_a_reader_unregisters_demand_and_allows_another_read() {
    let (sparse, cipher, reference, _) = progressive_fixture("bytes-cancel");
    let cancel = Arc::new(AtomicBool::new(false));
    let callback = cancel.clone();
    let reader = std::thread::spawn(move || {
        read_range_cancellable("bytes-cancel", 720, 10, Some(10), || {
            callback.load(Ordering::Relaxed)
        })
    });
    let cache = crate::cache();
    let progressive = cache
        .get("bytes-cancel", 720)
        .unwrap()
        .progressive
        .lock()
        .clone()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !progressive.demand.has_waiters() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(progressive.demand.has_waiters());
    cancel.store(true, Ordering::Relaxed);
    assert!(matches!(
        reader.join().unwrap(),
        Err(StreamReadError::Cancelled)
    ));
    assert!(!progressive.demand.has_waiters());
    sparse.write(0, &cipher);
    assert_eq!(
        read_range("bytes-cancel", 720, 10, Some(10)).unwrap().bytes,
        reference[10..20]
    );
}

#[test]
fn replacing_a_waiting_entry_rejects_the_stale_lease() {
    register("bytes-replaced", stream(SparseBuffer::new(32), None, 32));
    let progressive = crate::cache()
        .get("bytes-replaced", 720)
        .unwrap()
        .progressive
        .lock()
        .clone()
        .unwrap();
    let reader = std::thread::spawn(|| read_range("bytes-replaced", 720, 0, Some(4)));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !progressive.demand.has_waiters() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(progressive.demand.has_waiters());
    crate::cache().remove("bytes-replaced", 720);
    assert!(matches!(
        reader.join().unwrap(),
        Err(StreamReadError::Replaced)
    ));
    assert!(!progressive.demand.has_waiters());
}

#[test]
fn missing_entries_fail_without_waiting_and_qualities_are_independent() {
    assert!(matches!(
        read_range("bytes-missing", 720, 0, Some(1)),
        Err(StreamReadError::Missing { .. })
    ));
    assert_eq!(total_len("bytes-missing", 720), None);
    ready_stream("bytes-quality", 720, 100);
    ready_stream("bytes-quality", 1080, 200);
    assert_eq!(total_len("bytes-quality", 720), Some(100));
    assert_eq!(total_len("bytes-quality", 1080), Some(200));
}

#[test]
fn progressive_identity_reads_return_original_bytes() {
    let data: Vec<_> = (0..600).map(|i| (i % 251) as u8).collect();
    let sparse = SparseBuffer::new(data.len() as u64);
    sparse.write(0, &data);
    register("bytes-identity", stream(sparse, None, data.len() as u64));
    assert_eq!(
        read_range("bytes-identity", 720, 100, Some(100))
            .unwrap()
            .bytes,
        data[100..200]
    );
}

#[test]
fn registration_does_not_resurrect_evicted_entries_or_replace_whole_data() {
    let cache = StreamCache::default();
    assert!(!cache.set_progressive("registration", 720, stream(SparseBuffer::new(4), None, 4)));
    cache.store("registration", 720, &[1; 8]);
    assert!(!cache.set_progressive("registration", 720, stream(SparseBuffer::new(4), None, 4)));
}

#[test]
fn reader_demand_keeps_fifo_order_skips_ready_ranges_and_clears_on_unregister() {
    let demand = ReaderDemand::new();
    let sparse = SparseBuffer::new(100);
    sparse.write(0, &[0; 10]);
    let ready = demand.register(&[(0, 10)]);
    let first = demand.register(&[(60, 70)]);
    let second = demand.register(&[(5, 20)]);
    assert_eq!(demand.first_wanted(&sparse), Some(60));
    demand.unregister(first);
    assert_eq!(demand.first_wanted(&sparse), Some(10));
    demand.unregister(second);
    assert_eq!(demand.first_wanted(&sparse), None);
    assert!(demand.has_waiters());
    demand.unregister(ready);
    assert!(!demand.has_waiters());
}
