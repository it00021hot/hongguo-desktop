use super::*;
use crate::StreamCache;
use std::sync::atomic::AtomicBool;

#[test]
fn zero_window_means_till_end_not_underflow() {
    assert_eq!(open_range_end(0, 0, 1_000), 1_000);
    assert_eq!(open_range_end(500, 0, 1_000), 1_000);
    assert_eq!(open_range_end(0, 100, 1_000), 100);
    assert_eq!(open_range_end(900, 100, 1_000), 1_000);
    assert_eq!(open_range_end(999, 100, 1_000), 1_000);
}

fn ready_stream(vid: &str, definition: u32, len: usize) {
    let c = crate::cache();
    c.remove(vid, definition);
    c.store(
        vid,
        definition,
        &(0..len).map(|i| (i % 251) as u8).collect::<Vec<u8>>(),
    );
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[test]
fn serves_full_body_after_fill() {
    ready_stream("v-full", 1080, 4096);
    let (status, headers, body) = serve("v-full", 1080, None).expect("整文件请求应成功");
    assert_eq!(status, 200);
    assert_eq!(body.len(), 4096);
    assert_eq!(header(&headers, "Content-Type"), Some("video/mp4"));
    assert_eq!(header(&headers, "Accept-Ranges"), Some("bytes"));
    assert_eq!(header(&headers, "Content-Length"), Some("4096"));
    assert_eq!(header(&headers, "Cache-Control"), Some("no-store"));
}

#[test]
fn serves_closed_range_for_seeking() {
    ready_stream("v-closed", 1080, 4096);
    let (status, headers, body) = serve("v-closed", 1080, Some("bytes=100-199")).unwrap();
    assert_eq!(status, 206);
    assert_eq!(body.len(), 100);
    assert_eq!(
        header(&headers, "Content-Range"),
        Some("bytes 100-199/4096")
    );
    assert_eq!(header(&headers, "Content-Length"), Some("100"));
}

#[test]
fn open_range_returns_everything_from_the_offset() {
    ready_stream("v-open", 1080, 4096);
    let (status, headers, body) = serve("v-open", 1080, Some("bytes=0-")).unwrap();
    assert_eq!(status, 206);
    assert_eq!(body.len(), 4096);
    assert_eq!(header(&headers, "Content-Range"), Some("bytes 0-4095/4096"));
    assert_eq!(header(&headers, "Content-Length"), Some("4096"));
}

#[test]
fn open_range_after_a_seek_also_reaches_the_end() {
    ready_stream("v-seek", 1080, 4096);
    let (status, _, _) = serve("v-seek", 1080, Some("bytes=1146880-")).unwrap();
    assert_eq!(status, 416, "超出总长的偏移仍应是不可满足");

    ready_stream("v-seek", 1080, 4096);
    let (status, headers, body) = serve("v-seek", 1080, Some("bytes=3000-")).unwrap();
    assert_eq!(status, 206);
    assert_eq!(body.len(), 1096);
    assert_eq!(
        header(&headers, "Content-Range"),
        Some("bytes 3000-4095/4096")
    );
}

#[test]
fn unknown_stream_is_an_error_not_a_hang() {
    let err = serve("never-prepared", 1080, Some("bytes=0-")).expect_err("未准备的流应报错");
    assert!(err.contains("没有正在准备的在线流"), "实际: {err}");
}

#[test]
fn out_of_bounds_range_is_416() {
    ready_stream("v-416", 1080, 100);
    let (status, headers, body) = serve("v-416", 1080, Some("bytes=500-600")).unwrap();
    assert_eq!(status, 416);
    assert_eq!(header(&headers, "Content-Range"), Some("bytes */100"));
    assert!(body.is_empty());
}

#[test]
fn serve_fails_fast_when_entry_is_replaced_midwait() {
    let c = crate::cache();
    c.remove("v-stale", 1080);
    c.set_definitions("v-stale", 1080, &[]);
    let handle = std::thread::spawn(|| serve("v-stale", 1080, Some("bytes=0-")));
    std::thread::sleep(Duration::from_millis(300));
    c.remove("v-stale", 1080);
    c.set_definitions("v-stale", 1080, &[]);
    let started = std::time::Instant::now();
    let result = handle.join().expect("serve 线程不应 panic");
    assert!(result.is_err(), "被替换的流应报错而不是返回空数据");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "应快速失败，不该等满 30s 超时"
    );
}

#[test]
fn switching_quality_leaves_the_previous_one_serving_its_own_bytes() {
    const FILL_1080: u8 = 0xA1;
    const FILL_720: u8 = 0xB2;
    let c = crate::cache();
    c.remove("v-switch", 1080);
    c.remove("v-switch", 720);
    c.store("v-switch", 1080, &[FILL_1080; 512]);
    c.store("v-switch", 720, &[FILL_720; 256]);

    let (status, _, body) = serve("v-switch", 1080, Some("bytes=0-99")).unwrap();
    assert_eq!(status, 206);
    assert!(
        body.iter().all(|b| *b == FILL_1080),
        "1080 应继续供自己那份字节"
    );

    let (status, _, body) = serve("v-switch", 720, Some("bytes=0-99")).unwrap();
    assert_eq!(status, 206);
    assert!(body.iter().all(|b| *b == FILL_720), "720 应供自己那份字节");
}

#[test]
fn a_definition_that_was_never_fetched_is_not_served() {
    ready_stream("v-only-1080", 1080, 256);
    let err = serve("v-only-1080", 720, Some("bytes=0-")).expect_err("没下过的档位应报错");
    assert!(err.contains("没有正在准备的在线流"), "实际: {err}");
}

fn progressive_fixture(vid: &str, def: u32) -> (Arc<SparseBuffer>, Vec<u8>, Vec<u8>, u64) {
    use hongguo_video::decrypt_buffer::decrypt_mp4_buffer;
    use hongguo_video::streaming::StreamingPlan;
    use hongguo_video::streaming_tests::assemble;

    let (_plain, cipher, key) = assemble(true);
    let reference = decrypt_mp4_buffer(&cipher, &key).expect("参照明文应可得");
    let len = cipher.len() as u64;
    let (abs, size) = hongguo_video::streaming::locate_moov(&cipher, 0, len).expect("应定位 moov");
    let region = &cipher[abs as usize..(abs + size) as usize];
    let plan = StreamingPlan::build(region, abs, len, &key).expect("计划应构建成功");
    let plain_len = plan.plain_len();
    let sparse = SparseBuffer::new(len);
    let prog = ProgressiveStream {
        sparse: sparse.clone(),
        plan: Some(Arc::new(plan)),
        plain_len,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    };
    let c = crate::cache();
    c.remove(vid, def);
    assert!(c.begin_fetch(vid, def));
    c.end_fetch(vid, def);
    assert!(c.set_progressive(vid, def, Arc::new(prog)));
    (sparse, cipher, reference, plain_len)
}

#[test]
fn progressive_serves_decrypted_range_as_data_arrives() {
    let (sparse, cipher, reference, plain_len) = progressive_fixture("v-prog", 720);

    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        sparse.write(0, &cipher);
    });
    let (status, headers, body) = serve("v-prog", 720, Some("bytes=0-99")).expect("应成功");
    writer.join().unwrap();

    assert_eq!(status, 206);
    assert_eq!(body, &reference[0..100], "应答必须是解密后的明文");
    assert_eq!(
        header(&headers, "Content-Range"),
        Some(format!("bytes 0-99/{plain_len}").as_str())
    );
}

#[test]
fn progressive_open_range_returns_a_window_from_the_offset() {
    let (sparse, cipher, reference, plain_len) = progressive_fixture("v-prog-open", 720);
    sparse.write(0, &cipher);

    let (status, headers, body) = serve("v-prog-open", 720, Some("bytes=30-")).unwrap();
    assert_eq!(status, 206);
    assert_eq!(body, &reference[30..]);
    assert_eq!(
        header(&headers, "Content-Range"),
        Some(format!("bytes 30-{}/{plain_len}", plain_len - 1).as_str())
    );
}

#[test]
fn progressive_beyond_plain_len_is_416() {
    let (sparse, cipher, _reference, plain_len) = progressive_fixture("v-prog-416", 720);
    sparse.write(0, &cipher);

    let (status, headers, body) = serve("v-prog-416", 720, Some("bytes=999999999-")).unwrap();
    assert_eq!(status, 416);
    assert_eq!(
        header(&headers, "Content-Range"),
        Some(format!("bytes */{plain_len}").as_str())
    );
    assert!(body.is_empty());
}

#[test]
fn progressive_identity_stream_serves_raw_bytes() {
    let c = crate::cache();
    let data: Vec<u8> = (0..600u32).map(|i| (i % 251) as u8).collect();
    let sparse = SparseBuffer::new(data.len() as u64);
    sparse.write(0, &data);
    let prog = ProgressiveStream {
        sparse,
        plan: None,
        plain_len: data.len() as u64,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    };
    c.remove("v-prog-raw", 720);
    assert!(c.begin_fetch("v-prog-raw", 720));
    c.end_fetch("v-prog-raw", 720);
    assert!(c.set_progressive("v-prog-raw", 720, Arc::new(prog)));

    let (status, _, body) = serve("v-prog-raw", 720, Some("bytes=100-199")).unwrap();
    assert_eq!(status, 206);
    assert_eq!(body, &data[100..200]);
}

#[test]
fn progressive_registers_demand_while_waiting() {
    let (sparse, cipher, _reference, _len) = progressive_fixture("v-prog-demand", 720);

    let server = std::thread::spawn(move || {
        let r = serve("v-prog-demand", 720, Some("bytes=10-19"));
        (r.is_ok(), r.map(|(_, _, b)| b.len()).unwrap_or(0))
    });
    std::thread::sleep(Duration::from_millis(150));

    let c = crate::cache();
    let prog = c
        .get("v-prog-demand", 720)
        .and_then(|e| e.progressive.lock().clone())
        .expect("渐进条目应在");
    assert!(prog.demand.has_waiters(), "等待期间应留下需求登记");
    assert_eq!(
        prog.demand.first_wanted(&prog.sparse),
        Some(10),
        "第一个需求洞应在缺口起点"
    );

    sparse.write(0, &cipher);
    let (ok, len) = server.join().unwrap();
    assert!(ok);
    assert_eq!(len, 10);
    assert!(!prog.demand.has_waiters(), "应答完成后登记应注销");
}

#[test]
fn cancellable_serve_exits_promptly_and_unregisters_demand() {
    let (sparse, cipher, _reference, _len) = progressive_fixture("v-prog-cancel", 720);

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_cb = Arc::clone(&cancel);
    let server = std::thread::spawn(move || {
        serve_cancellable("v-prog-cancel", 720, Some("bytes=10-19"), || {
            cancel_cb.load(Ordering::Relaxed)
        })
    });
    std::thread::sleep(Duration::from_millis(150));
    let c = crate::cache();
    let prog = c
        .get("v-prog-cancel", 720)
        .and_then(|e| e.progressive.lock().clone())
        .expect("渐进条目应在");
    assert!(prog.demand.has_waiters(), "等待期间应留下需求登记");

    let started = std::time::Instant::now();
    cancel.store(true, Ordering::Relaxed);
    let result = server.join().expect("serve 线程不应 panic");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "取消应快速生效，实际 {:?}",
        started.elapsed()
    );
    assert!(result.is_err(), "取消的读取应报错而不是返回数据");

    assert!(!prog.demand.has_waiters(), "取消后读者登记应注销");

    sparse.write(0, &cipher);
    let (status, _, body) = serve("v-prog-cancel", 720, Some("bytes=10-19")).expect("应成功");
    assert_eq!(status, 206);
    assert_eq!(body, &cipher[10..20]);
}

#[test]
fn progressive_is_reported_ready_before_any_data() {
    let c = crate::cache();
    c.remove("v-prog-ready", 720);
    let sparse = SparseBuffer::new(16);
    let prog = ProgressiveStream {
        sparse,
        plan: None,
        plain_len: 16,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    };
    assert!(c.begin_fetch("v-prog-ready", 720));
    c.end_fetch("v-prog-ready", 720);
    assert!(c.set_progressive("v-prog-ready", 720, Arc::new(prog)));
    assert!(c.ready("v-prog-ready", 720).is_some(), "注册即就绪");
}

#[test]
fn progressive_registration_does_not_resurrect_an_evicted_entry() {
    let c = crate::cache();
    c.remove("v-prog-gone", 720);
    let prog = ProgressiveStream {
        sparse: SparseBuffer::new(16),
        plan: None,
        plain_len: 16,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    };
    assert!(!c.set_progressive("v-prog-gone", 720, Arc::new(prog)));
    assert!(c.get("v-prog-gone", 720).is_none(), "不得复活条目");
}

#[test]
fn set_progressive_yields_to_whole_mode_data() {
    let c = StreamCache::default();
    c.store("v-race", 720, &[1u8; 8]);
    let prog = ProgressiveStream {
        sparse: SparseBuffer::new(4),
        plan: None,
        plain_len: 4,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    };
    assert!(!c.set_progressive("v-race", 720, Arc::new(prog)));
}

use crate::read::StreamReadError;
use crate::{read_range, read_range_cancellable, total_len};

#[test]
fn byte_reads_whole_mode_range_like_serve() {
    ready_stream("v-bytes-whole", 1080, 4096);
    assert_eq!(total_len("v-bytes-whole", 1080), Some(4096));

    let chunk = read_range("v-bytes-whole", 1080, 100, Some(50)).expect("闭区间应成功");
    let reference: Vec<u8> = (100..150).map(|i| (i % 251) as u8).collect();
    assert_eq!(chunk.total, 4096);
    assert_eq!(chunk.start, 100);
    assert_eq!(chunk.bytes, reference);

    let (status, _, body) = serve("v-bytes-whole", 1080, Some("bytes=100-149")).unwrap();
    assert_eq!(status, 206);
    assert_eq!(body, chunk.bytes);
}

#[test]
fn byte_read_range_clamps_at_end_and_allows_eof_read() {
    ready_stream("v-bytes-clamp", 1080, 4096);

    let tail = read_range("v-bytes-clamp", 1080, 4090, Some(100)).expect("越过末尾应截断");
    assert_eq!(tail.bytes.len(), 6, "只应回末尾剩余字节");
    assert_eq!(tail.total, 4096);

    let eof = read_range("v-bytes-clamp", 1080, 4096, None).expect("EOF 读应成功");
    assert!(eof.bytes.is_empty());
    assert_eq!(eof.start, 4096);

    let empty = read_range("v-bytes-clamp", 1080, 4090, Some(0)).expect("零长读应成功");
    assert!(empty.bytes.is_empty());

    let error = read_range("v-bytes-clamp", 1080, 4097, Some(1)).expect_err("越界应报错");
    assert!(matches!(
        error,
        StreamReadError::OutOfRange {
            start: 4097,
            total: 4096
        }
    ));
}

#[test]
fn byte_progressive_len_is_known_before_any_data() {
    let (sparse, cipher, reference, plain_len) = progressive_fixture("v-bytes-prog", 720);
    assert_eq!(total_len("v-bytes-prog", 720), Some(plain_len));

    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        sparse.write(0, &cipher);
    });
    let chunk = read_range("v-bytes-prog", 720, 10, Some(20)).expect("渐进读应成功");
    writer.join().unwrap();
    assert_eq!(chunk.bytes, &reference[10..30]);
    assert_eq!(chunk.total, plain_len);
}

#[test]
fn byte_progressive_read_cancel_unregisters_demand() {
    let (sparse, cipher, _reference, _len) = progressive_fixture("v-bytes-cancel", 720);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_cb = Arc::clone(&cancel);
    let reader = std::thread::spawn(move || {
        read_range_cancellable("v-bytes-cancel", 720, 10, Some(10), || {
            cancel_cb.load(Ordering::Relaxed)
        })
    });
    std::thread::sleep(Duration::from_millis(150));

    let c = crate::cache();
    let prog = c
        .get("v-bytes-cancel", 720)
        .and_then(|entry| entry.progressive.lock().clone())
        .expect("渐进条目应在");
    assert!(prog.demand.has_waiters(), "等待期间应留下读者登记");

    cancel.store(true, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let error = reader
        .join()
        .expect("读取线程不应 panic")
        .expect_err("应被取消");
    assert!(matches!(error, StreamReadError::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "取消应立即生效，实际 {:?}",
        started.elapsed()
    );
    assert!(!prog.demand.has_waiters(), "取消后读者登记应注销");

    sparse.write(0, &cipher);
    let chunk = read_range("v-bytes-cancel", 720, 10, Some(10)).expect("取消后应可重读");
    assert_eq!(chunk.bytes, &cipher[10..20]);
}

#[test]
fn byte_missing_entry_is_an_error_not_a_hang() {
    let error = read_range("v-bytes-never", 720, 0, Some(1)).expect_err("未准备的流应报错");
    assert!(matches!(error, StreamReadError::Missing { .. }));
    assert_eq!(total_len("v-bytes-never", 720), None);
}

#[test]
fn byte_reads_identity_progressive_stream() {
    let c = crate::cache();
    let data: Vec<u8> = (0..600u32).map(|i| (i % 251) as u8).collect();
    let sparse = SparseBuffer::new(data.len() as u64);
    sparse.write(0, &data);
    let prog = ProgressiveStream {
        sparse,
        plan: None,
        plain_len: data.len() as u64,
        demand: ReaderDemand::new(),
        cdn_url: "https://cdn/example.mp4".into(),
        media_cache_key: None,
        proxy: Default::default(),
    };
    c.remove("v-bytes-raw", 720);
    assert!(c.begin_fetch("v-bytes-raw", 720));
    c.end_fetch("v-bytes-raw", 720);
    assert!(c.set_progressive("v-bytes-raw", 720, Arc::new(prog)));

    assert_eq!(total_len("v-bytes-raw", 720), Some(600));
    let chunk = read_range("v-bytes-raw", 720, 100, Some(100)).unwrap();
    assert_eq!(chunk.bytes, &data[100..200]);
}

#[test]
fn reader_demand_serves_fifo_and_clears_on_unregister() {
    let d = ReaderDemand::new();
    let sparse = SparseBuffer::new(100);
    sparse.write(0, &[0u8; 10]);

    let t1 = d.register(&[(0, 10)]);
    let t2 = d.register(&[(80, 90)]);
    assert_eq!(
        d.first_wanted(&sparse),
        Some(80),
        "就绪的登记跳过，轮到后到者"
    );

    d.unregister(t2);
    assert_eq!(d.first_wanted(&sparse), None, "注销后不再指向它");
    assert!(d.has_waiters(), "t1 仍在");
    d.unregister(t1);
    assert!(!d.has_waiters(), "全部注销后无等待者");
}

#[test]
fn reader_demand_lands_inside_mid_range_hole() {
    let d = ReaderDemand::new();
    let sparse = SparseBuffer::new(100);
    sparse.write(10, &[0u8; 10]);
    sparse.write(25, &[0u8; 5]);
    d.register(&[(10, 40)]);
    assert_eq!(d.first_wanted(&sparse), Some(20));
    sparse.write(20, &[0u8; 5]);
    assert_eq!(
        d.first_wanted(&sparse),
        Some(30),
        "补齐后应指到区间内下一个洞"
    );
}

#[test]
fn reader_demand_fifo_never_starves_early_reader() {
    let d = ReaderDemand::new();
    let sparse = SparseBuffer::new(100);
    d.register(&[(60, 70)]);
    d.register(&[(5, 10)]);
    assert_eq!(
        d.first_wanted(&sparse),
        Some(60),
        "先到者优先，即使它的区间更靠后"
    );
}
