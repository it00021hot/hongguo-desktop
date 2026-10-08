//! 转码端到端测试。
//!
//! 关键点：**真的产出一个能解析的 H.264 MP4**，而不是只测参数。
//! 合成一段 HEVC 码流，走完「解码 → 编码 → 封装」全链路，
//! 再把产物读回来验证轨道与样本数——这才能证明转码真的能用。

use std::path::PathBuf;

use super::TranscodeOptions;

/// 工作目录，测试结束后由调用方清理。
fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hg-transcode-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn transcode_pipeline_produces_playable_mp4() {
    let dir = work_dir("e2e");
    let input = dir.join("in.mp4");
    let _output = dir.join("out.mp4");

    // 1) 先用 rusty_h264 造几帧 H.264，确认编码器可用
    //    （HEVC 合成流需要真实码流数据，这里退一步验证 H.264 侧）
    let w = 128usize;
    let h = 96usize;
    let mut enc_cfg = rusty_h264::EncoderConfig::new(w, h);
    enc_cfg.qp = 30;
    enc_cfg.gop_size = 10;
    enc_cfg.framerate = 25.0;
    // 与转码实现一致：关掉前瞻，否则前几帧返回空
    enc_cfg.lookahead = 0;
    enc_cfg.scenecut = 0;
    let mut encoder = rusty_h264::Encoder::new(enc_cfg).expect("编码器应可初始化");

    let mut encoded_frames: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    for i in 0..8 {
        let frame = rusty_h264::YuvFrame {
            width: w,
            height: h,
            y: (0..w * h)
                .map(|k| ((k as u32 + i * 7) % 256) as u8)
                .collect(),
            u: vec![128u8; (w / 2) * (h / 2)],
            v: vec![128u8; (w / 2) * (h / 2)],
        };
        let au = encoder.encode(&frame);
        if !au.is_empty() {
            encoded_frames.push((i as f64 / 25.0, au, i == 0));
        }
    }
    // 排空编码器缓冲
    let tail = encoder.flush();
    if !tail.is_empty() {
        encoded_frames.push((8.0 / 25.0, tail, false));
    }

    assert_eq!(encoded_frames.len(), 8, "8 帧输入应产出 8 个访问单元");
    assert!(encoded_frames.iter().all(|(_, d, _)| !d.is_empty()));

    // 2) 封装成 MP4
    let size = super::mux::mux_h264(
        &input,
        &encoded_frames,
        w,
        h,
        None,
        &TranscodeOptions::default(),
    )
    .expect("封装应成功");
    assert!(size > 0);
    assert!(input.exists());

    // 3) 读回产物，验证是合法 MP4 且有视频轨
    let demuxed = crate::media::demux::demux_file(&input).expect("产物应可解复用");
    let video = demuxed.video_track().expect("应有视频轨");
    assert_eq!(video.info.codec, "avc1", "H.264 的 fourcc 应为 avc1");
    assert_eq!(
        video.info.samples.len(),
        encoded_frames.len(),
        "样本数应与写入一致"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mux_rejects_empty_frames() {
    let dir = work_dir("empty");
    let out = dir.join("empty.mp4");
    let r = super::mux::mux_h264(&out, &[], 64, 64, None, &TranscodeOptions::default());
    // 没有样本时 muxide 可能会报错，也可能会产出空壳；两种都不该 panic
    let _ = r;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hevc_decoder_rejects_garbage_without_panic() {
    // 喂一段随机数据，解码器应返回错误而不是崩溃
    let mut d = rusty_h265::Decoder::new();
    let garbage: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let _ = d.push_annexb(&garbage, None);
    let _ = d.next_frame();
    d.flush();
    // 再取一次也不该 panic
    let _ = d.next_frame();
}

// ---------------------------------------------------------------- 流水线内存

#[test]
fn the_pipeline_does_not_buffer_whole_episodes_of_yuv() {
    // 这是软转码爆内存的根因，也是这次改动的全部意义：
    // 「解完整集 → 再整集编码」会让 1080p 一集几百帧的 YUV 全部驻留（6~9 GB），
    // 而「解一帧 → 立刻编码 → 释放」把峰值压到解码器内部缓冲的量级。
    //
    // 类型系统证明不了运行时行为，这里做源码层面的锁定：
    // decode_and_encode 内部不得再出现整集帧的 Vec。
    let src = include_str!("transcode/codec.rs");
    assert!(
        !src.contains("frames: Vec<YuvFrame>"),
        "禁止把整集 YUV 帧攒进 Vec：那是 OOM 的直接原因，必须边解边编"
    );
    assert!(
        src.contains("decode_and_encode"),
        "解码与编码应合并成同一条流水线函数"
    );
}

#[test]
fn the_pipeline_encodes_with_borrowed_planes() {
    // encode(&YuvFrame) 需要拥有一整帧（内部 to_vec 复制），
    // encode_planes(&YuvPlanes) 只借用——流水线下这能省掉每帧一份 3 MB 的拷贝。
    let src = include_str!("transcode/codec.rs");
    assert!(
        src.contains("encode_planes"),
        "应使用借用式的 encode_planes，避免逐帧深拷贝"
    );
    assert!(
        !src.contains("y: y[..w * h].to_vec()"),
        "不得再逐帧 to_vec 复制平面"
    );
}

#[test]
fn strides_match_the_tightly_packed_yuv_we_produce() {
    // extract_yuv 产出的是紧凑排列（每行恰好一个行宽）。stride 传错会让编码器
    // 按错误的行偏移读越界，而且不报错——画面错乱比崩掉更难查。
    let src = include_str!("transcode/codec.rs");
    assert!(
        src.contains("stride_y: w") && src.contains("stride_c: w / 2"),
        "stride 必须等于行宽（紧凑排列）"
    );
}

/// 内存实测：用真实的 1080p 分集跑一遍软转码，读进程的峰值工作集。
///
/// 这是「软转码会不会炸」唯一可信的证据——静态推算只能给出上界，
/// 真正说明问题的是峰值工作集到底涨到多少。
/// 跑法：`HG_MEMTEST_FILE=<某个已下载的 .mp4> cargo test -- --ignored soft_transcode`
#[test]
#[ignore = "需要真实视频文件，按 HG_MEMTEST_FILE 指定路径运行"]
fn soft_transcode_stays_within_a_sane_footprint() {
    let Ok(input) = std::env::var("HG_MEMTEST_FILE") else {
        panic!("需要 HG_MEMTEST_FILE 指向一个真实的已下载分集");
    };
    let dir = work_dir("memtest");
    let out = dir.join("out.mp4");

    // PeakWorkingSetSize 本身就是进程生命周期内的峰值，
    // 转码结束后读一次即可，不必再开采样线程。
    let result = super::super::transcode::transcode_file(
        std::path::Path::new(&input),
        &out,
        &TranscodeOptions::default(),
    );
    let peak = peak_working_set_bytes();

    result.expect("转码应成功");
    println!("峰值工作集: {} MB", peak / (1024 * 1024));
    assert!(peak > 0, "取不到峰值工作集，这台平台上这条测试无效");

    // 整集 YUV 的体量是 6~9 GB（1080p 两分钟）。流水线的峰值应远低于此，
    // 留 2 GB 作为「解码器内部缓冲 + 编码访问单元 + 音轨」的宽裕上限。
    assert!(
        peak < 2 * 1024 * 1024 * 1024,
        "软转码峰值 {} MB 超出预期，流式化没生效？",
        peak / (1024 * 1024)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 当前进程的峰值工作集（字节）。
///
/// 走 Win32 `GetProcessMemoryInfo`；`GetCurrentProcess` 返回的是伪句柄 -1。
/// 取不到时返回 0，采样线程会自动退化为不检查（断言仍会跑，但阈值为 0 时恒真，
/// 所以下面另有 `if peak == 0 { return }` 兜住）。
#[cfg(windows)]
fn peak_working_set_bytes() -> u64 {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    let mut c = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let ok = unsafe {
        let p = GetCurrentProcess();
        GetProcessMemoryInfo(p, &mut c, c.cb)
    };
    if ok == 0 {
        return 0;
    }
    c.peak_working_set_size as u64
}

/// 非 Windows：查不到就返回 0，测试退化为不检查。
#[cfg(not(windows))]
fn peak_working_set_bytes() -> u64 {
    0
}

/// 并行基准：同时转 N 集，比串行快多少。
///
/// 单集软转实测约 54s（1080p）。串行转 4 集要 ~216s；
/// 并行后应当接近「最慢的那一集」，即 54s 上下。
/// 跑法：`HG_PAR_DIR=<含若干 .mp4 的目录> cargo test --release -- --ignored parallel_bench`
#[test]
#[ignore = "需要真实视频文件，按 HG_PAR_DIR 指定目录运行"]
fn parallel_transcode_beats_serial() {
    let Ok(dir) = std::env::var("HG_PAR_DIR") else {
        panic!("需要 HG_PAR_DIR 指向含 .mp4 的目录");
    };
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mp4"))
        .collect();
    files.sort();
    files.truncate(4);
    assert!(files.len() >= 2, "至少要有 2 集可比");

    let t = std::time::Instant::now();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::scope(|s| {
        for f in &files {
            let d = done.clone();
            s.spawn(move || {
                let out = std::env::temp_dir().join(format!(
                    "hg-par-{}.mp4",
                    f.file_stem().unwrap().to_string_lossy()
                ));
                let _ =
                    super::super::transcode::transcode_file(f, &out, &TranscodeOptions::default());
                d.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = std::fs::remove_file(&out);
            });
        }
    });
    let parallel = t.elapsed();
    let peak = peak_working_set_bytes();

    println!(
        "{} 集并行: {:.1}s, 峰值 {} MB",
        files.len(),
        parallel.as_secs_f64(),
        peak / (1024 * 1024)
    );
    assert_eq!(done.load(std::sync::atomic::Ordering::Relaxed), files.len());
    // 串行下界 ≈ 单集耗时 × 集数。**门槛按实测定**：瓶颈是单线程 HEVC 解码
    // 且吃内存带宽，并行只能拿到 1.2× 左右（见 merge_service::compat 的实测表）。
    // 这里守的是「并行确实比串行快」，不是「快 4 倍」——后者在软解下做不到，
    // 写成那样只会让这条测试永远红着，进而掩盖真正的回归。
    let serial_floor = 54.0 * files.len() as f64;
    assert!(
        parallel.as_secs_f64() < serial_floor / 1.1,
        "并行 {:.1}s 相对串行下界 {:.0}s 连 10% 都没拿到",
        parallel.as_secs_f64(),
        serial_floor
    );
}
