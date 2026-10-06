//! 播放兼容兜底：解码不出来时转成 H.264 再播。
//!
//! 平台给的是 HEVC，WebView2 能不能解完全看系统——装了「HEVC 视频扩展」
//! 就能看，没装就是**有声音没画面**。这里在确认解不出来之后，把这一集转成
//! H.264 落进兼容缓存，播放器改播转码产物。
//!
//! 为什么值得做：黑屏是「看不出为什么也改不了」的死局，而兜底只在这一集上
//! 付一次转码的代价（之后命中缓存）。机器上没有 ffmpeg 时回落到纯 Rust 软解。
//!
//! 进度按已编码秒数上报到界面——转码一集要几十秒，没有进度用户只能等。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::service::transcode_service::cache;

/// 兜底转码的结果。
pub struct CompatPlay {
    /// 转码产物的可播放地址（`hongguo-local://`）
    pub url: String,
    /// 命中的已有缓存
    pub cached: bool,
    /// 实际用的转码后端
    pub backend: String,
    /// 转码耗时（毫秒），命中缓存时为 0
    pub elapsed_ms: u128,
}

/// 给一集准备兼容格式产物，返回可播放地址。
///
/// `source` 是这一集的**明文 MP4**：已下载的给本地路径，没下载的给整集明文字节。
/// `on_progress` 收到 0–100 的百分比。
pub async fn ensure_compat(
    series_id: &str,
    vid_index: u32,
    source: CompatSource<'_>,
    on_progress: &(dyn Fn(f64) + Send + Sync),
) -> AppResult<CompatPlay> {
    if let Some(p) = cache::cached_path(series_id, vid_index) {
        on_progress(100.0);
        let url = crate::protocol::local::local_play_url(&p.to_string_lossy())
            .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
        return Ok(CompatPlay {
            url,
            cached: true,
            backend: crate::media::backend_info().transcode_with,
            elapsed_ms: 0,
        });
    }

    let target = cache::cache_file(series_id, vid_index);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = crate::service::download_service::worker::temp_path_for(&target);
    let started = std::time::Instant::now();

    // 1) 落到一个可解复用的明文文件
    let (plain, is_temp) = match source {
        CompatSource::LocalFile(p) => (p.to_path_buf(), false),
        CompatSource::PlainBytes(bytes) => {
            std::fs::write(&temp, &bytes).map_err(|e| AppError::Io(e.to_string()))?;
            (temp.clone(), true)
        }
    };

    // 百分比要拿总时长才折得出来。读不出来就报 0——宁可显示不确定，
    // 也不要给一个编出来的数字。
    let total_secs = video_seconds(&plain);
    let report = |secs: f64| {
        let pct = match total_secs {
            Some(t) if t > 0.0 => (secs / t * 100.0).clamp(0.0, 100.0),
            _ => 0.0,
        };
        on_progress(pct);
    };
    report(0.0);

    // 2) 转码。ffmpeg 优先（含硬编码器），没有就回纯 Rust 软解。
    let used_ffmpeg = match crate::media::ffmpeg::h264_encoder() {
        Some(encoder) => {
            let req = crate::media::ffmpeg::TranscodeRequest {
                input: &plain,
                output: &target,
                scale_to: None,
                on_progress: Some(&report),
            };
            match crate::media::ffmpeg::transcode_with_ffmpeg(&req, &encoder) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("[Compat] ffmpeg 转码失败，回落软解: {e}");
                    false
                }
            }
        }
        None => false,
    };

    if !used_ffmpeg {
        // 软解只认 HEVC，别的编码在这里就该说清楚
        crate::media::codec_probe::ensure_softdecode_supported(&plain)?;
        if is_temp {
            let _ = std::fs::remove_file(&plain);
        }
        crate::media::transcode::transcode_file(&plain, &target, &Default::default())?;
    } else if is_temp {
        // 源明文只是中间产物，产物已经是 H.264 了
        let _ = std::fs::remove_file(&plain);
    }

    on_progress(100.0);
    cache::trim();
    let elapsed = started.elapsed();
    let backend = crate::media::backend_info();
    let url = crate::protocol::local::local_play_url(&target.to_string_lossy())
        .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
    log::info!(
        "[Compat] 第 {vid_index} 集就绪：{}，耗时 {elapsed:?}",
        backend.transcode_with
    );
    Ok(CompatPlay {
        url,
        cached: false,
        backend: backend.transcode_with,
        elapsed_ms: elapsed.as_millis(),
    })
}

/// 视频轨时长（秒）。读不出来返回 `None`。
fn video_seconds(path: &Path) -> Option<f64> {
    let tracks = crate::media::demux::demux_file(path).ok()?;
    let v = tracks.video_track()?;
    (v.info.media_timescale > 0)
        .then(|| v.info.media_duration as f64 / v.info.media_timescale as f64)
}

/// 这一集的明文从哪来。
pub enum CompatSource<'a> {
    /// 已下载的本地文件
    LocalFile(&'a PathBuf),
    /// 未下载：整集取回 + 解密后的明文字节
    PlainBytes(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_cached_compat_file_short_circuits() {
        // 命中缓存时不该碰源，也不再起一次转码
        let dir = std::env::temp_dir().join(format!("hg-compat-hit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = crate::store::paths::ScopedDataDir::new(&dir);
        let p = cache::cache_file("s", 1);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![7u8; 200 * 1024]).unwrap();

        let got = ensure_compat("s", 1, CompatSource::PlainBytes(Vec::new()), &|_| {})
            .await
            .expect("命中缓存不该失败");
        assert!(got.cached, "应当命中缓存");
        assert_eq!(got.elapsed_ms, 0, "命中缓存不耗时间");
        assert!(
            got.url.starts_with(&format!(
                "{}/f/",
                crate::protocol::scheme_base(crate::protocol::LOCAL_SCHEME)
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
        let _scope = crate::store::paths::ScopedDataDir::new(&dir);
        let missing = dir.join("nope.mp4");
        let r = ensure_compat("s2", 1, CompatSource::LocalFile(&missing), &|_| {}).await;
        assert!(r.is_err(), "源不存在时必须报错，不能产出空文件");
        assert!(!cache::cache_file("s2", 1).exists(), "失败时不应留下产物");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
