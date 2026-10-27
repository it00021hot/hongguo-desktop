//! 播放兼容兜底：解码不出来时转成 H.264 再播。
//!
//! 平台给的是 HEVC，WebView2 能不能解完全看系统——装了「HEVC 视频扩展」
//! 就能看，没装就是**有声音没画面**。这里在确认解不出来之后，把这一集转成
//! H.264 落进兼容缓存，播放器改播转码产物。
//!
//! 为什么值得做：黑屏是「看不出为什么也改不了」的死局，而兜底只在这一集上
//! 付一次转码的代价（之后命中缓存）。转码走 [`pipeline::transcode`] 的统一
//! 分流链（平台硬编 → ffmpeg）。
//!
//! 进度按已编码秒数上报到界面——转码一集要几十秒，没有进度用户只能等。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::app_state::AppState;
use crate::events::ServiceEvent;
use crate::transcode_service::cache;
use hongguo_common::{AppError, AppResult};

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
        let url = hongguo_protocol::local::local_play_url(&p.to_string_lossy())
            .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
        return Ok(CompatPlay {
            url,
            cached: true,
            backend: backend_label(hongguo_media_platform::capability::selected_backend()),
            elapsed_ms: 0,
        });
    }

    let target = cache::cache_file(series_id, vid_index);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = plaintext_temp_path_for(&target);
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

    // 2) 转码。分流链（平台硬编 → ffmpeg）在 pipeline 里只有一份，
    //    这里必须走同一个入口——曾在这里复刻过一条 ffmpeg→软编的私有分流，
    //    平台硬编接不进来，兼容播放永远拿不到硬编速度。
    let result = crate::transcode_service::pipeline::transcode(
        series_id,
        vid_index,
        &plain,
        None,
        Some(&report),
    );

    // 源明文只是中间产物，转码成功或失败都要清理，避免残留敏感媒体数据。
    if is_temp {
        let _ = std::fs::remove_file(&plain);
    }
    let result = result?;

    on_progress(100.0);
    let elapsed = started.elapsed();
    let url = hongguo_protocol::local::local_play_url(&result.output_path)
        .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
    let backend = backend_label(result.backend);
    log::info!("[Compat] 第 {vid_index} 集就绪：{backend}，耗时 {elapsed:?}");
    Ok(CompatPlay {
        url,
        cached: false,
        backend,
        elapsed_ms: elapsed.as_millis(),
    })
}

/// 后端标识，给界面提示用。
///
/// 没有可用后端时（平台编码器不可用且没装 ffmpeg），产物只可能是此前转好的
/// 缓存——这时报「none」，不编一个后端名出来。
fn backend_label(backend: Option<hongguo_media::Backend>) -> String {
    match backend {
        Some(b) => b.decoder_label().to_string(),
        None => "none".to_string(),
    }
}

/// 兼容播放的明文输入临时文件，必须与转码流水线的输出临时文件区分。
fn plaintext_temp_path_for(target: &Path) -> PathBuf {
    let stem = target
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("compat");
    target.with_file_name(format!("{stem}.plain.tmp"))
}

/// 视频轨时长（秒）。读不出来返回 `None`。
fn video_seconds(path: &Path) -> Option<f64> {
    let tracks = hongguo_media::demux::demux_file(path).ok()?;
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

/// 兼容转码进度事件的阶段（前端 `compatProgressSchema` 镜像）。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompatPhase {
    Downloading,
    Transcoding,
    Ready,
}

/// 兼容转码进度事件载荷（`compat-play-progress`）。
///
/// 与前端 `compatProgressSchema` 两端镜像，改字段同一 commit 改另一端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatProgressEvent {
    /// 缓存键 `{series_id}:{vid_index}`
    pub key: String,
    pub percent: f64,
    pub phase: CompatPhase,
}

/// 兜底转码的结果（前端直接消费）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatPlayResult {
    pub url: String,
    pub cached: bool,
    pub backend: String,
    pub elapsed_ms: u128,
}

/// 把一集转成 H.264 供本机解不了的机器播放（编排自 tauri 命令平移）。
///
/// 播放器确认「有声音没画面」后调它；转好的产物落进兼容缓存，同一集只转一次。
/// 已下载的走本地文件，没下载的现取现解；进度经事件总线（
/// `compat-play-progress`）送达壳。
pub async fn compat_for_playback(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
) -> AppResult<CompatPlayResult> {
    let key = format!("{series_id}:{vid_index}");
    let events = state.events().clone();
    let emit = |percent: f64, phase: CompatPhase| {
        events.publish(ServiceEvent::CompatProgress(CompatProgressEvent {
            key: key.clone(),
            percent,
            phase,
        }));
    };

    // 优先用已下载的本地文件：不用再走网络，也不用等取流
    let local: Option<PathBuf> = state
        .queue()
        .completed_path(series_id, vid_index)
        .map(PathBuf::from);

    let result = if let Some(p) = local {
        ensure_compat(series_id, vid_index, CompatSource::LocalFile(&p), &|pct| {
            emit(pct, CompatPhase::Transcoding)
        })
        .await
    } else {
        let Some(vid) = vid else {
            return Err(AppError::InvalidArgs("既没有本地文件，也没有 vid".into()));
        };
        let settings = state.settings();
        let env = state.api_env();
        // 转码源必须保持全量候选：HEVC-only 的剧集兜底转码还要拿 HEVC 源
        let play = hongguo_api::play_url::fetch_play_url(
            &vid,
            None,
            hongguo_api::stream_pick::DecoderCapability::Full,
            &env,
        )
        .await?;
        // 取流占前 20%，转码占后 80%：两段都动，但用户看到的是一条连续的进度
        emit(0.0, CompatPhase::Downloading);
        let plain = crate::play_service::online::fetch_plain(&play, &settings).await?;
        ensure_compat(
            series_id,
            vid_index,
            CompatSource::PlainBytes(plain),
            &|pct| emit(20.0 + pct * 0.8, CompatPhase::Transcoding),
        )
        .await
    };

    emit(100.0, CompatPhase::Ready);
    let r = result?;
    Ok(CompatPlayResult {
        url: r.url,
        cached: r.cached,
        backend: r.backend,
        elapsed_ms: r.elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_cached_compat_file_short_circuits() {
        // 命中缓存时不该碰源，也不再起一次转码
        let dir = std::env::temp_dir().join(format!("hg-compat-hit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
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
        let r = ensure_compat("s2", 1, CompatSource::LocalFile(&missing), &|_| {}).await;
        assert!(r.is_err(), "源不存在时必须报错，不能产出空文件");
        assert!(!cache::cache_file("s2", 1).exists(), "失败时不应留下产物");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plaintext_source_temp_is_distinct_from_ffmpeg_output_temp() {
        let target = cache::cache_file("s", 22);
        // 兼容播放的明文输入不能与流水线为最终输出创建的临时文件重名。
        let source = plaintext_temp_path_for(&target);
        let ffmpeg_output = hongguo_common::utils::path::temp_path_for(&target);
        assert_ne!(source, ffmpeg_output, "ffmpeg 会把自己的临时输出误认成输入");
    }

    #[tokio::test]
    async fn plaintext_source_temp_is_removed_when_transcode_fails() {
        let dir =
            std::env::temp_dir().join(format!("hg-compat-temp-cleanup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _scope = hongguo_store::paths::ScopedDataDir::new(&dir);
        let target = cache::cache_file("cleanup", 1);
        let source_temp = plaintext_temp_path_for(&target);

        let result = ensure_compat(
            "cleanup",
            1,
            CompatSource::PlainBytes(vec![1, 2, 3]),
            &|_| {},
        )
        .await;

        assert!(result.is_err(), "无效媒体数据应当转码失败");
        assert!(!source_temp.exists(), "失败后不能残留明文临时文件");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
