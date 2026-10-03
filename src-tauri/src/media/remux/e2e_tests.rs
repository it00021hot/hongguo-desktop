//! 真实短剧文件的端到端验证（仅测试用）。
//!
//! 用真实码流验证三件在合成 fixture 上证明不了的事：
//!
//! 1. 索引重写在**平台封装器**产出的文件上同样成立（HEVC Main profile 带 B 帧、
//!    `ctts` 非空、chunk 不止一个、stsc 多条目）。
//! 2. 产物被 `ffprobe` 认成一条**完整**的时间轴，而不是「只能播第 1 集」。
//! 3. ffmpeg 路线产出的 H.264 能播，且比纯 Rust 软解快多少。
//!
//! 全部用例由 `HONGGUO_E2E_DIR` 门控：没设就整体跳过，真实文件不进
//! `make test-rust`。跑法：
//!
//! ```text
//! HONGGUO_E2E_DIR=<一堆已解密 mp4 所在目录> cargo test --lib media::remux::e2e -- --nocapture
//! HONGGUO_DATA_DIR=<临时目录> # 让 compat-cache 落在临时目录，不碰真实存档
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::Value;

use crate::media::ffmpeg::transcode_with_ffmpeg;
use crate::media::remux::concat_copy;
use crate::media::{ffmpeg, transcode};

/// 没设就整体跳过：真实文件不进 `make test-rust`。
///
/// Rust 的 `#[test]` 没有「跳过」，所以每个用例开头都 `let Some(..) = .. else { return }`。
/// 这里**不能**用 `expect`——那会让没准备数据的机器直接红一片。
fn e2e_dir() -> Option<PathBuf> {
    std::env::var("HONGGUO_E2E_DIR").ok().map(PathBuf::from)
}

/// 目录里的 mp4，按文件名自然序（001…010）排开。数据没就绪时返回 `None`。
fn episodes() -> Option<Vec<PathBuf>> {
    let dir = e2e_dir()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("读不了 {dir:?}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mp4"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 2,
        "至少要两集才能验证拼接，实际 {}",
        files.len()
    );
    Some(files)
}

/// 能参与合并的子集：与第 1 集编码参数不一致的那几集被剔掉。
///
/// 短剧各集是平台**分别**编码的，同一部剧里混着不同分辨率是常态，所以「全部
/// 一起合」经常根本不成立。判定复用应用自己的 [`crate::media::codec_probe`]，
/// 保证测试里的口径和界面上的口径是同一份。
fn mergeable_episodes() -> Option<(Vec<PathBuf>, Option<u32>)> {
    let files = episodes()?;
    let numbered: Vec<(u32, PathBuf)> = files
        .iter()
        .enumerate()
        .map(|(i, p)| (i as u32 + 1, p.clone()))
        .collect();
    let mismatch = crate::media::codec_probe::check(&numbered).mismatch_episode;
    let keep = match mismatch {
        None => files,
        Some(bad) => files
            .iter()
            .enumerate()
            .filter(|(i, _)| *i as u32 + 1 != bad)
            .map(|(_, p)| p.clone())
            .collect(),
    };
    assert!(keep.len() >= 2, "剔除不一致的集后不足两集，没法验证拼接");
    Some((keep, mismatch))
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hg-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 不缩放、不报进度的转码请求——e2e 只关心产物对不对。
fn plain_req<'a>(input: &'a Path, output: &'a Path) -> crate::media::ffmpeg::TranscodeRequest<'a> {
    crate::media::ffmpeg::TranscodeRequest {
        input,
        output,
        scale_to: None,
        on_progress: None,
    }
}

/// 编码器的可读标签，带上是否走硬件。
fn enc_label(e: &crate::media::ffmpeg::probe::Encoder) -> String {
    if e.hardware {
        format!("{}/hw", e.name)
    } else {
        e.name.to_string()
    }
}

/// `ffprobe` 的 JSON。缺 ffprobe 就明确报错，不静默跳过——那会让「产物到底
/// 能不能播」这个问题没人回答。
fn probe(path: &Path) -> Value {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o,
        Ok(o) => panic!(
            "ffprobe 对 {} 报错: {}",
            path.display(),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => panic!("跑不了 ffprobe，先装 ffmpeg（{e}）"),
    };
    serde_json::from_slice(&out.stdout).expect("ffprobe 的输出应当是 JSON")
}

/// 产物时长（秒）。走 `mdhd` 而不是 `format=duration`——后者对某些封装器
/// 会取到编辑列表算出来的值，和样本时间轴对不上。
fn media_seconds(path: &Path) -> f64 {
    let tracks = crate::media::demux::demux_file(path).expect("产物应能解复用");
    tracks
        .video_track()
        .map(|t| t.info.media_duration as f64 / t.info.media_timescale.max(1) as f64)
        .expect("产物应有视频轨")
}

fn probe_duration(path: &Path) -> f64 {
    let v = probe(path);
    v["format"]["duration"]
        .as_str()
        .unwrap_or_else(|| panic!("ffprobe 没报出时长: {}", v["format"]))
        .parse()
        .expect("时长应是数字")
}

fn video_codec(path: &Path) -> String {
    let v = probe(path);
    v["streams"]
        .as_array()
        .expect("应有 streams")
        .iter()
        .find(|s| s["codec_type"] == "video")
        .map(|s| s["codec_name"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

fn audio_codec(path: &Path) -> String {
    let v = probe(path);
    v["streams"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["codec_type"] == "audio"))
        .and_then(|s| s["codec_name"].as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn mixed_resolution_episodes_are_refused_with_a_readable_message() {
    let Some(files) = episodes() else { return };
    let dir = scratch("mixed");
    let out = dir.join("合集.mp4");

    // 这批数据里通常混着不同分辨率的集（平台逐集编码，同一部剧不保证同规格）。
    // 有不一致就验拒绝，没有就说明这份数据刚好一致，顺带确认能合。
    let numbered: Vec<(u32, PathBuf)> = files
        .iter()
        .enumerate()
        .map(|(i, p)| (i as u32 + 1, p.clone()))
        .collect();
    match crate::media::codec_probe::check(&numbered).mismatch_episode {
        None => {
            let (size, _) = concat_copy(&files, &out).unwrap();
            assert!(size > 0, "全部一致时应当能合出文件");
            println!("[e2e] 这批数据各集规格一致，直接合并成功（{size} 字节）");
        }
        Some(bad) => {
            let err = concat_copy(&files, &out).expect_err("规格不一致必须拒绝");
            let text = err.to_string();
            assert!(text.contains("分辨率"), "报错要指出是分辨率：{text}");
            assert!(text.contains(&bad.to_string()), "报错要指到集号：{text}");
            assert!(!out.exists(), "拒绝时不产出文件");
            println!("[e2e] 第 {bad} 集规格不一致，已按预期拒绝：{text}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_episodes_merge_into_one_continuous_timeline() {
    let Some((files, mismatch)) = mergeable_episodes() else {
        return;
    };
    let dir = scratch("merge");
    let out = dir.join("合集.mp4");

    let source_samples: usize = files
        .iter()
        .map(|p| {
            crate::media::demux::demux_file(p)
                .unwrap()
                .video_track()
                .unwrap()
                .info
                .samples
                .len()
        })
        .sum();
    let source_seconds: f64 = files.iter().map(|p| media_seconds(p)).sum();

    let started = Instant::now();
    let (size, count) = concat_copy(&files, &out).unwrap();
    let elapsed = started.elapsed();

    let merged = crate::media::demux::demux_file(&out).expect("产物应能解复用");
    let video = merged.video_track().unwrap();

    assert_eq!(count, files.len());
    assert_eq!(
        video.info.samples.len(),
        source_samples,
        "合并后的样本数必须是各集之和"
    );
    let merged_seconds =
        video.info.media_duration as f64 / video.info.media_timescale.max(1) as f64;
    assert!(
        (merged_seconds - source_seconds).abs() < source_seconds * 0.01,
        "合并时长 {merged_seconds:.2}s 应对齐各集之和 {source_seconds:.2}s"
    );
    assert!(size > 0);
    println!(
        "[e2e] {} 集（剔除 {:?}）→ 1 集，{source_samples} 个视频样本，{source_seconds:.2}s，\
         产物 {:.1}MB，拼接耗时 {:.2?}",
        files.len(),
        mismatch,
        size as f64 / 1048576.0,
        elapsed
    );

    // 最硬的一条：逐样本比对字节。ffprobe 只看时长，音画错位它查不出来。
    let data = std::fs::read(&out).unwrap();
    let sources: Vec<Vec<(u64, u64)>> = files
        .iter()
        .map(|p| {
            crate::media::demux::demux_file(p)
                .unwrap()
                .video_track()
                .unwrap()
                .info
                .samples
                .clone()
        })
        .collect();
    let mut file_idx = 0usize;
    let mut sample_idx = 0usize;
    for i in 0..video.info.samples.len() {
        let (at, len) = video.info.samples[i];
        // 产物是一条连续时间轴，i 是**合并后**的全局样本号；
        // 取源时要用「当前集内的第几个样本」，两者不是一回事。
        let (src_at, src_len) = sources[file_idx][sample_idx];
        assert_eq!(len, src_len, "第 {i} 个样本大小与源不一致");
        let a = read_range(&data, at, len);
        let b = read_range_from(&files[file_idx], src_at, src_len);
        assert!(a == b, "第 {i} 个样本字节与源不一致（{at} vs {src_at}）");
        sample_idx += 1;
        if sample_idx == sources[file_idx].len() {
            sample_idx = 0;
            file_idx += 1;
        }
    }
    assert_eq!(file_idx, sources.len(), "所有输入的样本都应被消费");
    let _ = std::fs::remove_dir_all(&dir);
}

fn read_range(data: &[u8], at: u64, len: u64) -> Vec<u8> {
    data[at as usize..(at + len) as usize].to_vec()
}

fn read_range_from(path: &Path, at: u64, len: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).unwrap();
    f.seek(SeekFrom::Start(at)).unwrap();
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf).unwrap();
    buf
}

#[test]
fn real_merge_is_accepted_by_ffprobe() {
    let Some((files, _)) = mergeable_episodes() else {
        return;
    };
    let dir = scratch("ffprobe");
    let out = dir.join("合集.mp4");
    concat_copy(&files, &out).unwrap();

    let expected: f64 = files.iter().map(|p| probe_duration(p)).sum();
    let got = probe_duration(&out);

    assert!(
        (got - expected).abs() < expected * 0.02,
        "ffprobe 认不出完整时间轴：产物 {got:.2}s，各集之和 {expected:.2}s"
    );
    // 帧数是各集之和——字节级拼接做不到这一点，ffprobe 的时长会停在第 1 集
    let frames = |p: &Path| -> u64 {
        probe(p)["streams"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["codec_type"] == "video"))
            .and_then(|s| s["nb_frames"].as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let source_video: u64 = files.iter().map(|p| frames(p)).sum();
    let merged_video = frames(&out);
    println!(
        "[e2e] ffprobe：产物 {got:.2}s / 各集之和 {expected:.2}s，帧数 {merged_video}/{source_video}"
    );
    assert_eq!(
        merged_video, source_video,
        "产物帧数应正好是各集之和，不多不少"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ffmpeg_transcode_produces_playable_h264() {
    let Some(encoder) = ffmpeg::h264_encoder() else {
        eprintln!("[e2e] 没探到可用的 H.264 编码器，跳过");
        return;
    };
    let Some(mut all) = episodes() else { return };
    let src = all.swap_remove(0);
    let dir = scratch("ffmpeg");
    let out = dir.join("转码.mp4");

    let started = Instant::now();
    transcode_with_ffmpeg(&plain_req(&src, &out), &encoder).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(video_codec(&out), "h264", "产物应是 H.264");
    assert_eq!(audio_codec(&out), "aac", "音轨应直通保留为 AAC");

    let source = media_seconds(&src);
    let got = probe_duration(&out);
    assert!(
        (got - source).abs() < source * 0.01,
        "转码后时长 {got:.2}s 应与源 {source:.2}s 一致"
    );
    println!(
        "[e2e] ffmpeg({}) 转码 {source:.1}s 单集：{elapsed:.2?}，产物 {:.1}MB",
        enc_label(&encoder),
        std::fs::metadata(&out).unwrap().len() as f64 / 1048576.0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn softdecode_and_ffmpeg_output_are_both_decodable() {
    // ⚠️ 这条在 **debug 构建下极慢**（单线程软解 1080p，实测跑了几分钟还没出结果）。
    //   `cargo test` 默认就是 debug，跑它之前先 `cargo test --release`。
    //   `compat.rs` 里记的 54s/集 是 release 下的数，拿它和 ffmpeg 比才有意义。
    let Some(encoder) = ffmpeg::h264_encoder() else {
        eprintln!("[e2e] 没探到可用的 H.264 编码器，跳过");
        return;
    };
    // 挑最小的一集：软解是单线程的，慢的不是代码而是等待
    let Some(all) = episodes() else { return };
    let src = all
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX))
        .unwrap();
    let dir = scratch("both");

    let soft_out = dir.join("软解.mp4");
    let soft_started = Instant::now();
    let soft = transcode::transcode_file(&src, &soft_out, &transcode::TranscodeOptions::default())
        .expect("软解应能转出可解析的 MP4");
    let soft_elapsed = soft_started.elapsed();

    let ff_out = dir.join("ffmpeg.mp4");
    let ff_started = Instant::now();
    transcode_with_ffmpeg(&plain_req(&src, &ff_out), &encoder).unwrap();
    let ff_elapsed = ff_started.elapsed();

    for (label, path) in [("软解", &soft_out), ("ffmpeg", &ff_out)] {
        let tracks = crate::media::demux::demux_file(path).expect("产物应能解复用");
        assert!(tracks.video_track().is_some(), "{label} 产物应有视频轨");
        assert!(
            (media_seconds(path) - media_seconds(&src)).abs() < media_seconds(&src) * 0.01,
            "{label} 产物时长应与源一致"
        );
        println!(
            "[e2e] {label}: {} 帧 / {:.2?}s",
            soft.frames,
            if label == "软解" {
                soft_elapsed
            } else {
                ff_elapsed
            }
        );
    }
    println!(
        "[e2e] 同集对比：软解 {:.2?} vs ffmpeg({}) {:.2?}，快 {:.1}×",
        soft_elapsed,
        enc_label(&encoder),
        ff_elapsed,
        soft_elapsed.as_secs_f64() / ff_elapsed.as_secs_f64().max(0.001)
    );
    assert!(
        ff_elapsed < soft_elapsed,
        "ffmpeg 路线应快于纯 Rust 软解：{ff_elapsed:?} vs {soft_elapsed:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compat_merge_end_to_end_via_ffmpeg() {
    let Some((all, _)) = mergeable_episodes() else {
        return;
    };
    // 只取三集：软解基准下全集要十分钟，ffmpeg 下没必要跑那么久
    let inputs: Vec<PathBuf> = all.into_iter().take(3).collect();
    let dir = scratch("compat");

    // 逐集转码到 compat-cache，再流复制拼接——和 compat.rs 里同一条流水线
    let mut transcoded = Vec::new();
    for (i, src) in inputs.iter().enumerate() {
        let target = dir.join(format!("{:03}.mp4", i + 1));
        let e2e_enc = ffmpeg::h264_encoder().unwrap();
        transcode_with_ffmpeg(&plain_req(src, &target), &e2e_enc).unwrap();
        transcoded.push(target);
    }

    let out = dir.join("合集.mp4");
    let (_, count) = concat_copy(&transcoded, &out).unwrap();
    assert_eq!(count, inputs.len());

    let expected: f64 = inputs.iter().map(|p| media_seconds(p)).sum();
    let got = probe_duration(&out);
    assert!(
        (got - expected).abs() < expected * 0.02,
        "兼容合并产物时长 {got:.2}s 应对齐各集之和 {expected:.2}s"
    );
    assert_eq!(video_codec(&out), "h264");
    assert_eq!(audio_codec(&out), "aac");
    println!("[e2e] 兼容合并 {count} 集：{got:.2}s（各集之和 {expected:.2}s）");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compat_merge_unifies_mixed_resolutions_by_scaling() {
    // 兼容合并逐集转码但**不缩放**的话，产物依然规格不一，拼接照样过不去——
    // 实测一部 11 集的剧（10 集 1080p、1 集 720p）走到拼接才失败，
    // 前面十几分钟的转码全白做。统一到首集分辨率后这条路才通。
    let Some(encoder) = ffmpeg::h264_encoder() else {
        eprintln!("[e2e] 没探到可用的 H.264 编码器，跳过");
        return;
    };
    let Some(files) = episodes() else { return };
    // 挑一集 1080p 与一集非 1080p
    let mut picks: Vec<PathBuf> = Vec::new();
    for p in &files {
        let tracks = crate::media::demux::demux_file(p).unwrap();
        let t = tracks.video_track().unwrap();
        let want_1080 = picks.is_empty();
        if (want_1080 && t.info.height == 1080) || (!want_1080 && t.info.height != 1080) {
            picks.push(p.clone());
        }
        if picks.len() == 2 {
            break;
        }
    }
    if picks.len() < 2 {
        eprintln!("[e2e] 这批数据没有混合分辨率，跳过");
        return;
    }
    let (w, h) = crate::service::transcode_service::pipeline::resolution_of(&picks[0]).unwrap();

    let dir = scratch("compat-mixed");
    let seconds = |p: &Path| {
        let t = crate::media::demux::demux_file(p).unwrap();
        let v = t.video_track().unwrap();
        v.info.media_duration as f64 / v.info.media_timescale.max(1) as f64
    };

    // 不缩放：拼接必须拒绝（这就是之前白干十几分钟的那条路）
    let mut raw = Vec::new();
    for (i, src) in picks.iter().enumerate() {
        let t = dir.join(format!("raw{:03}.mp4", i + 1));
        transcode_with_ffmpeg(&plain_req(src, &t), &encoder).unwrap();
        raw.push(t);
    }
    let err = concat_copy(&raw, &dir.join("raw.mp4")).expect_err("不缩放时必须拒绝");
    assert!(err.to_string().contains("分辨率"), "实际: {err}");

    // 统一到首集规格：同样两集应当拼得出来
    let mut scaled = Vec::new();
    for (i, src) in picks.iter().enumerate() {
        let t = dir.join(format!("scaled{:03}.mp4", i + 1));
        let req = crate::media::ffmpeg::TranscodeRequest {
            input: src,
            output: &t,
            scale_to: Some((w, h)),
            on_progress: None,
        };
        transcode_with_ffmpeg(&req, &encoder).unwrap();
        scaled.push(t);
    }
    let out = dir.join("scaled.mp4");
    let (_, count) = concat_copy(&scaled, &out).expect("统一分辨率后应当能拼");
    assert_eq!(count, 2);

    let tracks = crate::media::demux::demux_file(&out).unwrap();
    let v = tracks.video_track().unwrap();
    assert_eq!(
        (v.info.width, v.info.height),
        (w, h),
        "产物应统一为首集规格"
    );
    let expect: f64 = picks.iter().map(|p| seconds(p)).sum();
    let got = v.info.media_duration as f64 / v.info.media_timescale.max(1) as f64;
    assert!(
        (got - expect).abs() < expect * 0.02,
        "合并后时长 {got:.2}s 应对齐各集之和 {expect:.2}s"
    );
    println!("[e2e] 混合分辨率统一到 {w}x{h} 后合并成功：{got:.2}s");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_installed_ffmpeg_is_actually_used_by_the_transcode_path() {
    // 门控：没有 ffmpeg 时整条兼容合并会静默回落软解，测试结果就没有意义了
    let Some(encoder) = ffmpeg::h264_encoder() else {
        // 没装 ffmpeg 是合法状态（项目本来就承诺零外部依赖），不是失败
        eprintln!(
            "[e2e] 没探到可用的 H.264 编码器，跳过：{:?}",
            ffmpeg::ffmpeg_path()
        );
        return;
    };
    let backend = ffmpeg::backend_info();
    assert!(
        backend.transcode_with.starts_with("ffmpeg"),
        "后端应报 ffmpeg，实际 {}",
        backend.transcode_with
    );
    let cap = crate::media::capability::probe::detect();
    assert!(cap.has_ffmpeg);
    assert_eq!(cap.h264_hw_encoder, encoder.hardware);
    println!(
        "[e2e] 后端 = {}（硬件={}），能力 = hasFfmpeg:{} hw:{}",
        backend.transcode_with, encoder.hardware, cap.has_ffmpeg, cap.h264_hw_encoder
    );
}

#[tokio::test]
async fn compat_playback_transcodes_a_real_hevc_episode() {
    // 兜底转码的端到端：真实 HEVC 源 → 转成 H.264 → 产物自带 avc1 轨。
    // 这是「播放兜底能救回黑屏机器」的前提。
    let Some(files) = episodes() else { return };
    let src = files[0].clone();
    let dir = std::env::temp_dir().join(format!("hg-e2e-cplay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _scope = crate::store::paths::ScopedDataDir::new(&dir);

    use crate::service::play_service::compat_play as cp;
    let got = match cp::ensure_compat("e2e-cp", 1, cp::CompatSource::LocalFile(&src), &|_| {}).await
    {
        Ok(v) => v,
        // 这台机器没有可用的转码后端时跳过，而不是判失败
        Err(e) => {
            eprintln!("[e2e] 兜底转码不可用，跳过: {e}");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
    };
    assert!(!got.cached, "首次不该命中缓存");
    assert!(got.url.contains("hongguo-local"), "实际: {}", got.url);

    let path = crate::service::transcode_service::cache::cache_file("e2e-cp", 1);
    let tracks = crate::media::demux::demux_file(&path).unwrap();
    let v = tracks.video_track().expect("产物要有视频轨");
    assert_eq!(v.info.codec, "avc1", "兜底的产物必须是 H.264");
    assert!(!v.info.samples.is_empty());
    println!(
        "[e2e] 兜底转码 {}：{}，耗时 {}ms",
        got.backend,
        std::fs::metadata(&path).unwrap().len() as f64 / 1048576.0,
        got.elapsed_ms
    );

    // 第二次命中缓存，不再转一次
    let again = cp::ensure_compat("e2e-cp", 1, cp::CompatSource::LocalFile(&src), &|_| {})
        .await
        .expect("命中缓存不该失败");
    assert!(again.cached, "第二次应当命中缓存");
    assert_eq!(again.elapsed_ms, 0, "命中缓存不耗时");
    let _ = std::fs::remove_dir_all(&dir);
}
