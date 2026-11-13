use super::*;

/// 真机冒烟：明文 bvc2 → 内置 Rust 解帧 → 产物须为 H.264。
#[tokio::test]
async fn plain_write_failure_after_open_reaches_a_terminal_state() {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.qa-b-scratch")
        })
        .join(format!("hg-write-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
    let target = cache::cache_file_for(
        "write-fail",
        1,
        hongguo_media_platform::RateProfile::Playback,
    );
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let temp = plaintext_temp_path_for(&target);
    std::fs::create_dir_all(&temp).unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        ensure_compat_progressive(
            "write-fail",
            1,
            CompatSource::PlainBytes(vec![1, 2, 3]),
            |_| {},
        ),
    )
    .await;
    assert!(result.expect("写入失败必须在超时前返回").is_err());
    assert!(!hongguo_protocol::grow::is_incomplete(&target));
    assert!(
        cache::cached_path_for(
            "write-fail",
            1,
            hongguo_media_platform::RateProfile::Playback
        )
        .is_none()
    );
    hongguo_protocol::grow::open(&target).expect("终态之后可以重新登记");
    hongguo_protocol::grow::fail(&target, "清理");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn qa_offline_local_play_refuses_when_switch_off() {
    let previous = std::env::var_os(hongguo_common::offline_qa::ENV);
    unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) };
    let store = hongguo_store::Store::open_memory().expect("mem");
    let state = std::sync::Arc::new(crate::app_state::AppStateInner::with_db(store));
    let err = qa_offline_local_play(&state, String::new())
        .await
        .expect_err("正式模式必须拒绝");
    assert!(matches!(err, AppError::Unsupported(_)), "实际={err:?}");
    match previous {
        Some(v) => unsafe { std::env::set_var(hongguo_common::offline_qa::ENV, v) },
        None => unsafe { std::env::remove_var(hongguo_common::offline_qa::ENV) },
    }
}

#[tokio::test]
#[ignore = "依赖 /tmp/hg-bytevc2-plain.mp4"]
async fn live_bytevc2_plain_transcodes_to_h264() {
    let plain = std::path::PathBuf::from("/tmp/hg-bytevc2-plain.mp4");
    assert!(
        plain.is_file(),
        "先跑 hongguo-api::probe_bytevc2_sample_live 落明文样本"
    );
    let demuxed = hongguo_media::demux::demux_file(&plain).expect("明文应能解复用");
    let codec = &demuxed.video_track().expect("应有视频轨").info.codec;
    assert!(
        hongguo_bytevc2::is_bytevc2(codec),
        "明文样本应是 bvc2，实际={codec}"
    );

    let sid = format!("bvc2-smoke-{}", std::process::id());
    let got = ensure_compat(&sid, 1, CompatSource::LocalFile(plain), |_| {})
        .await
        .expect("bytevc2 兼容转码应成功");
    assert!(!got.cached);
    let out = cache::cached_path_for(&sid, 1, hongguo_media_platform::RateProfile::Playback)
        .expect("转码产物应在缓存");
    let out_demux = hongguo_media::demux::demux_file(&out).expect("产物应能解复用");
    let out_codec = &out_demux.video_track().expect("产物应有视频轨").info.codec;
    assert!(
        hongguo_media::h264::is_h264(out_codec),
        "兼容产物须为 h264，实际={out_codec} backend={}",
        got.backend
    );
    let _ = std::fs::remove_file(&out);
    println!(
        "[bvc2-smoke] OK backend={} elapsed_ms={} out={}",
        got.backend,
        got.elapsed_ms,
        out.display()
    );
}

#[tokio::test]
async fn a_cached_compat_file_short_circuits() {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.qa-b-scratch")
        })
        .join(format!("hg-compat-hit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
    let p = cache::cache_file_for("s", 1, hongguo_media_platform::RateProfile::Playback);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, vec![7u8; 200 * 1024]).unwrap();
    cache::seal_complete(&p).unwrap();

    let got = ensure_compat("s", 1, CompatSource::PlainBytes(Vec::new()), |_| {})
        .await
        .expect("命中缓存不该失败");
    assert!(got.cached, "应当命中缓存");
    assert_eq!(got.elapsed_ms, 0, "命中缓存不耗时间");
    assert!(
        got.url.starts_with(&format!(
            "{}/f/",
            hongguo_protocol::scheme_base(hongguo_protocol::LOCAL_SCHEME)
        )),
        "实际: {}",
        got.url
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_missing_source_is_an_error_not_an_empty_artifact() {
    let dir = std::env::temp_dir().join(format!("hg-compat-miss-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
    let missing = dir.join("nope.mp4");
    let r = ensure_compat("s2", 1, CompatSource::LocalFile(missing), |_| {}).await;
    assert!(r.is_err(), "源不存在时必须报错，不能产出空文件");
    assert!(
        !cache::cache_file_for("s2", 1, hongguo_media_platform::RateProfile::Playback).exists(),
        "失败时不应留下产物"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn compatibility_cache_does_not_reuse_another_definition_or_a_legacy_result() {
    let dir = std::env::temp_dir().join(format!("hg-compat-def-{}", std::process::id()));
    let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
    for definition in [None, Some(1080)] {
        let path = cache::playback_file("s", 1, definition);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"other source").unwrap();
        cache::seal_complete(&path).unwrap();
    }

    let result = ensure_inner(
        "s",
        1,
        Some(720),
        CompatSource::LocalFile(dir.join("missing.mp4")),
        |_| {},
        |_| {},
        Delivery::Complete,
    )
    .await;
    assert!(matches!(result, Err(AppError::NotFound(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plaintext_source_temp_is_distinct_from_ffmpeg_output_temp() {
    let target = cache::cache_file_for("s", 22, hongguo_media_platform::RateProfile::Playback);
    let source = plaintext_temp_path_for(&target);
    let ffmpeg_output = hongguo_common::utils::path::temp_path_for(&target);
    assert_ne!(source, ffmpeg_output, "ffmpeg 会把自己的临时输出误认成输入");
}

#[tokio::test]
async fn plaintext_source_temp_is_removed_when_transcode_fails() {
    let dir = std::env::temp_dir().join(format!("hg-compat-temp-cleanup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
    let target = cache::cache_file_for("cleanup", 1, hongguo_media_platform::RateProfile::Playback);
    let source_temp = plaintext_temp_path_for(&target);

    let result = ensure_compat(
        "cleanup",
        1,
        CompatSource::PlainBytes(vec![1, 2, 3]),
        |_| {},
    )
    .await;

    assert!(result.is_err(), "无效媒体数据应当转码失败");
    assert!(!source_temp.exists(), "失败后不能残留明文临时文件");
    let _ = std::fs::remove_dir_all(&dir);
}
