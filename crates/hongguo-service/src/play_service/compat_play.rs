//! 播放兼容兜底：解码不出来时转成 H.264 再播（HEVC 能不能解完全看系统装没装「HEVC 视频扩展」）。
//! 转码走 `pipeline::transcode` 的统一分流链（平台硬编 → ffmpeg），产物落兼容缓存、同一集只转一次；
//! 进度按已编码秒数上报到界面。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::app_state::AppState;
use crate::events::ServiceEvent;
use crate::transcode_service::cache;
use hongguo_common::{AppError, AppResult};
mod session;

#[derive(Clone, Copy, PartialEq)]
enum Delivery {
    Complete,
    TauriPrefix,
    NativePrefix,
}
impl Delivery {
    fn prefix(self) -> bool {
        self != Self::Complete
    }
    fn bytevc2(self, native_available: bool) -> bool {
        self != Self::TauriPrefix && native_available
    }
}

/// 兜底转码的结果。
pub struct CompatPlay {
    /// 转码产物的可播放地址（`hongguo-local://`）
    pub url: String,
    /// 命中的已有缓存
    pub cached: bool,
    /// 实际用的转码后端
    pub backend: String,
    /// 转码耗时（毫秒）。提前交出首段时只计到首段落盘，命中缓存时为 0
    pub elapsed_ms: u128,
    /// 地址在整片写完前就可用，后续字节从同一路径继续读
    pub progressive: bool,
}

enum PrefixNote {
    Ready,
    Finished,
    Failed(String),
}

/// 给一集准备兼容格式产物，返回可播放地址。
///
/// `source` 是这一集的**明文 MP4**：已下载的给本地路径，没下载的给整集明文字节。
/// `on_progress` 收到 0–100 的百分比；owned 回调是因为转码段要整体进阻塞池（'static）。
pub async fn ensure_compat(
    series_id: &str,
    vid_index: u32,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> AppResult<CompatPlay> {
    ensure_inner(
        series_id,
        vid_index,
        None,
        source,
        on_progress,
        |_| {},
        Delivery::Complete,
    )
    .await
}

/// 首段可播就返回地址，转码在同一路径上继续。整片结束前缓存不会把半成品当成命中。
pub async fn ensure_compat_progressive(
    series_id: &str,
    vid_index: u32,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> AppResult<CompatPlay> {
    ensure_inner(
        series_id,
        vid_index,
        None,
        source,
        on_progress,
        |_| {},
        Delivery::TauriPrefix,
    )
    .await
}

async fn ensure_inner(
    series_id: &str,
    vid_index: u32,
    definition: Option<u32>,
    source: CompatSource,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
    on_settled: impl Fn(bool) + Send + Sync + 'static,
    delivery: Delivery,
) -> AppResult<CompatPlay> {
    let at_prefix = delivery.prefix();
    // 播放档与合并归档分文件（`.play.mp4`），命中/落盘都走 Playback，不能误用归档路径。
    let rate = hongguo_media_platform::RateProfile::Playback;
    let target = cache::playback_file(series_id, vid_index, definition);
    if cache::is_complete(&target) {
        on_progress(100.0);
        let url = hongguo_protocol::local::local_play_url(&target.to_string_lossy())
            .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
        return Ok(CompatPlay {
            url,
            cached: true,
            backend: backend_label(hongguo_media_platform::capability::selected_backend()),
            elapsed_ms: 0,
            progressive: false,
        });
    }

    if let Some(parent) = target.parent() {
        hongguo_store::paths::qa_create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let temp = plaintext_temp_path_for(&target);
    let started = std::time::Instant::now();

    // 写盘 + demux + 编码是分钟级阻塞，占专用阻塞池的线程，不能占 async 工作线程。
    let progress = Arc::new(on_progress);
    let progress_in_task = Arc::clone(&progress);
    let target_for_task = target.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    let tx_for_task = std::sync::Arc::clone(&tx);
    let handoff = at_prefix;
    let producer = if at_prefix {
        match session::begin(&target, delivery == Delivery::NativePrefix) {
            Ok(producer) => Some(producer),
            Err(error @ AppError::Busy(_)) if delivery == Delivery::NativePrefix => {
                return session::attach(&target).await?.ok_or(error);
            }
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    cache::discard_completion(&target);
    let cancel_for_task = producer.clone();
    let settled = on_settled;
    let task = crate::runtime::spawn_blocking(move || {
        let (plain, is_temp) = match source {
            CompatSource::LocalFile(p) => (p, false),
            CompatSource::PlainBytes(bytes) => {
                if let Err(err) = hongguo_store::paths::qa_write(&temp, &bytes) {
                    if handoff {
                        cache::discard_completion(&target_for_task);
                        hongguo_protocol::grow::fail(&target_for_task, err.to_string());
                        if let Some(sender) =
                            tx_for_task.lock().unwrap_or_else(|p| p.into_inner()).take()
                        {
                            let _ = sender.send(PrefixNote::Failed(err.to_string()));
                        }
                        if let Some(producer) = &cancel_for_task {
                            producer.settle(&Err::<(), _>(AppError::Io(err.to_string())));
                            session::clear(&target_for_task, producer);
                        }
                    }
                    return Err(AppError::Io(err.to_string()));
                }
                (temp.clone(), true)
            }
        };

        // 百分比要拿总时长才折得出来；读不出来报 0——宁可显示不确定，也不给一个编出来的数字。
        let total_secs = video_seconds(&plain);
        let report = move |secs: f64| {
            let pct = match total_secs {
                Some(t) if t > 0.0 => (secs / t * 100.0).clamp(0.0, 99.0),
                _ => 0.0,
            };
            progress_in_task(pct);
        };
        report(0.0);

        // 转码必须走 pipeline 的同一个入口——分流链只有一份，复刻私有分流
        // 会让平台硬编接不进来，兼容播放永远拿不到硬编速度。
        // 播放档：长边封顶 720p + 轻码率——归档档把 1080p 一分钟胀到 ~60MB，软编更是分钟级。
        let scale_to = crate::transcode_service::pipeline::resolution_of(&plain)
            .and_then(crate::transcode_service::pipeline::playback_scale);
        let committed = {
            let path = target_for_task.clone();
            let tx = std::sync::Arc::clone(&tx_for_task);
            let producer = cancel_for_task.clone();
            move |len: u64| {
                hongguo_protocol::grow::commit(&path, len);
                if let Some(producer) = &producer {
                    producer.ready();
                }
                if let Some(sender) = tx.lock().unwrap_or_else(|p| p.into_inner()).take() {
                    let _ = sender.send(PrefixNote::Ready);
                }
            }
        };
        let stop = {
            let producer = cancel_for_task.clone();
            move || {
                producer
                    .as_ref()
                    .is_some_and(|producer| producer.cancelled())
            }
        };
        let hooks = if handoff {
            crate::transcode_service::pipeline::TranscodeHooks {
                on_committed: Some(&committed),
                is_cancelled: Some(&stop),
            }
        } else {
            crate::transcode_service::pipeline::TranscodeHooks::default()
        };
        let mut result = crate::transcode_service::pipeline::transcode(
            &plain,
            &target_for_task,
            scale_to,
            rate,
            Some(&report),
            hooks,
        );
        if result.is_ok()
            && let Err(err) = cache::seal_complete(&target_for_task)
        {
            cache::discard_completion(&target_for_task);
            result = Err(AppError::Io(format!("完成标识没有写上: {err}")));
        }
        if handoff {
            match &result {
                Ok(_) => hongguo_protocol::grow::finish(&target_for_task),
                Err(AppError::Cancelled) => {
                    cache::discard_completion(&target_for_task);
                    hongguo_protocol::grow::cancel(&target_for_task);
                }
                Err(err) => {
                    cache::discard_completion(&target_for_task);
                    hongguo_protocol::grow::fail(&target_for_task, err.to_string());
                }
            }
            if let Some(producer) = &cancel_for_task {
                producer.settle(&result);
            }
            if let Some(sender) = tx_for_task.lock().unwrap_or_else(|p| p.into_inner()).take() {
                let note = match &result {
                    Ok(_) => PrefixNote::Finished,
                    Err(AppError::Cancelled) => PrefixNote::Failed("已取消".into()),
                    Err(err) => PrefixNote::Failed(err.to_string()),
                };
                let _ = sender.send(note);
            } else {
                settled(result.is_ok());
            }
        }

        // 源明文只是中间产物，转码成功或失败都要清理，避免残留敏感媒体数据。
        if is_temp {
            let _ = hongguo_store::paths::qa_remove_file(&plain);
        }
        // 新产物已进缓存：按「最多保留 3 集」的滚动窗口收敛一轮。合并路径不经这里——
        // 中途逐出会让拼接拿到断路径，它消费完就地删除。
        if result.is_ok() {
            cache::trim();
        }
        if let Some(producer) = &cancel_for_task {
            session::clear(&target_for_task, producer);
        }
        result
    });
    if at_prefix {
        let note = rx
            .await
            .map_err(|_| AppError::Media("转码任务在交出地址前结束".into()))?;
        let elapsed = started.elapsed();
        let url = hongguo_protocol::local::local_play_url(&target.to_string_lossy())
            .ok_or_else(|| AppError::Media("转码产物路径无法转成播放地址".into()))?;
        return match note {
            PrefixNote::Ready => {
                log::info!("[Compat] 第 {vid_index} 集首段可播，后续继续写入同一路径");
                Ok(CompatPlay {
                    url,
                    cached: false,
                    backend: backend_label(hongguo_media_platform::capability::selected_backend()),
                    elapsed_ms: elapsed.as_millis(),
                    progressive: true,
                })
            }
            PrefixNote::Finished => {
                progress(100.0);
                Ok(CompatPlay {
                    url,
                    cached: false,
                    backend: backend_label(hongguo_media_platform::capability::selected_backend()),
                    elapsed_ms: elapsed.as_millis(),
                    progressive: false,
                })
            }
            PrefixNote::Failed(message) => Err(AppError::Media(message)),
        };
    }
    let result = task
        .await
        .map_err(|e| AppError::Media(format!("兼容转码任务异常结束: {e}")))??;

    progress(100.0);
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
        progressive: false,
    })
}

/// 后端标识，给界面提示用。没有可用后端时报「none」，不编一个后端名出来
///（此时产物只可能是此前转好的缓存）。
/// 停止这一集还在写的兼容转码，并叫醒等在首段之后的读取。
pub fn cancel_compat(series_id: &str, vid_index: u32, definition: Option<u32>) {
    let path = cache::playback_file(series_id, vid_index, definition);
    session::cancel(&path);
}

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
pub enum CompatSource {
    /// 已下载的本地文件
    LocalFile(PathBuf),
    /// 未下载：整集取回 + 解密后的明文字节
    PlainBytes(Vec<u8>),
}

/// 兼容转码进度事件的阶段（前端 `compatProgressSchema` 镜像）。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompatPhase {
    Downloading,
    Transcoding,
    /// 首段可以读。不是整片结束，百分比不到 100。
    Ready,
    /// 整片成功落盘并且完成标识已写上。
    Completed,
    Failed,
}

/// 兼容转码进度事件载荷（`compat-play-progress`），与前端 `compatProgressSchema` 两端镜像，改字段同一 commit 改另一端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatProgressEvent {
    /// `{series_id}:{vid_index}`，在线档位再追加 `:{definition}`。
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
    /// 首段已经可播，整片还在同一路径上继续写
    pub progressive: bool,
}

/// 把一集转成 H.264 供本机解不了的机器播放。
/// 播放器确认「有声音没画面」后调它；已下载的走本地文件，没下载的现取现解；
/// 进度经事件总线（`compat-play-progress`）送达壳。
pub async fn compat_for_playback(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
    definition: Option<u32>,
) -> AppResult<CompatPlayResult> {
    compat_for_playback_with(
        state,
        series_id,
        vid_index,
        vid,
        definition,
        Delivery::Complete,
    )
    .await
}

/// Tauri 播放链：首段可播即返回。GPUI 仍走 [`compat_for_playback`]，等整片落完。
pub async fn compat_for_playback_prefix(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
    definition: Option<u32>,
) -> AppResult<CompatPlayResult> {
    compat_for_playback_with(
        state,
        series_id,
        vid_index,
        vid,
        definition,
        Delivery::TauriPrefix,
    )
    .await
}

/// Native video hosts consume a growing H.264 file. Keep the platform's real
/// ByteVC2 conversion capability and reuse the matching online plaintext stream.
pub async fn compat_for_native_playback_prefix(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
    definition: Option<u32>,
) -> AppResult<CompatPlayResult> {
    compat_for_playback_with(
        state,
        series_id,
        vid_index,
        vid,
        definition,
        Delivery::NativePrefix,
    )
    .await
}

async fn compat_for_playback_with(
    state: &AppState,
    series_id: &str,
    vid_index: u32,
    vid: Option<String>,
    definition: Option<u32>,
    delivery: Delivery,
) -> AppResult<CompatPlayResult> {
    crate::cold_start::wait().await;
    let local = if definition.is_none() {
        state
            .queue()
            .completed_path(series_id, vid_index)
            .filter(|path| Path::new(path).is_file())
            .map(PathBuf::from)
    } else {
        None
    };
    let key = match definition {
        Some(definition) => format!("{series_id}:{vid_index}:{definition}"),
        None => format!("{series_id}:{vid_index}"),
    };
    let events = state.events().clone();
    // 进度回调要随转码任务进阻塞池（'static）：emit 自持克隆，各分支再克隆出独立回调
    let emit = {
        let key = key.clone();
        let events = events.clone();
        move |percent: f64, phase: CompatPhase| {
            events.publish(ServiceEvent::CompatProgress(CompatProgressEvent {
                key: key.clone(),
                percent,
                phase,
            }));
        }
    };

    if delivery == Delivery::NativePrefix
        && let Some(play) =
            session::attach(&cache::playback_file(series_id, vid_index, definition)).await?
    {
        emit(
            if play.progressive { 99.0 } else { 100.0 },
            if play.progressive {
                CompatPhase::Ready
            } else {
                CompatPhase::Completed
            },
        );
        return Ok(CompatPlayResult {
            url: play.url,
            cached: play.cached,
            backend: play.backend,
            elapsed_ms: play.elapsed_ms,
            progressive: play.progressive,
        });
    }

    let result = if let Some(p) = local {
        let emit_tc = emit.clone();
        ensure_inner(
            series_id,
            vid_index,
            None,
            CompatSource::LocalFile(p),
            move |pct| emit_tc(pct, CompatPhase::Transcoding),
            {
                let emit = emit.clone();
                move |ok| {
                    if ok {
                        emit(100.0, CompatPhase::Completed);
                    } else {
                        emit(0.0, CompatPhase::Failed);
                    }
                }
            },
            delivery,
        )
        .await
    } else {
        let Some(vid) = vid else {
            return Err(AppError::InvalidArgs("既没有本地文件，也没有 vid".into()));
        };
        let settings = state.settings();
        let env = state.api_env();
        // Tauri 的 ByteVC2 源由直播放处理，兼容命令重选时也不能把它带入转码。
        let cap = hongguo_api::stream_pick::DecoderCapability::full().with_bytevc2(
            delivery.bytevc2(hongguo_media_platform::capability::bytevc2_decode_available()),
        );
        let play = hongguo_api::play_url::fetch_play_url(&vid, definition, cap, &env).await?;
        if definition.is_some_and(|expected| play.definition != expected) {
            return Err(AppError::Media("所选清晰度已不可用".into()));
        }
        // 取流占前 20%，转码占后 80%：两段都动，但用户看到的是一条连续的进度
        emit(0.0, CompatPhase::Downloading);
        let plain = if delivery == Delivery::NativePrefix {
            match crate::play_service::online::cached_plain(&vid, &play).await? {
                Some(bytes) => bytes,
                None => crate::play_service::online::fetch_plain(&play, &settings).await?,
            }
        } else {
            crate::play_service::online::fetch_plain(&play, &settings).await?
        };
        let emit_tc = emit.clone();
        let decoded = ensure_inner(
            series_id,
            vid_index,
            definition,
            CompatSource::PlainBytes(plain),
            move |pct| emit_tc(20.0 + pct * 0.8, CompatPhase::Transcoding),
            {
                let emit = emit.clone();
                move |ok| {
                    if ok {
                        emit(100.0, CompatPhase::Completed);
                    } else {
                        emit(0.0, CompatPhase::Failed);
                    }
                }
            },
            delivery,
        )
        .await;
        if let Err(err) = &decoded
            && !matches!(err, AppError::Cancelled | AppError::Busy(_))
            && hongguo_bytevc2::is_bytevc2(&play.codec)
        {
            log::warn!("[Compat] ByteVC2 内置解码失败，尝试同清晰度的其他编码: {err}");
            crate::cold_start::wait().await;
            let fallback = hongguo_api::play_url::fetch_play_url(
                &vid,
                Some(play.definition),
                hongguo_api::stream_pick::DecoderCapability::full(),
                &env,
            )
            .await?;
            if fallback.definition == play.definition
                && !hongguo_bytevc2::is_bytevc2(&fallback.codec)
            {
                let plain = crate::play_service::online::fetch_plain(&fallback, &settings).await?;
                let emit_tc = emit.clone();
                ensure_inner(
                    series_id,
                    vid_index,
                    definition,
                    CompatSource::PlainBytes(plain),
                    move |pct| emit_tc(20.0 + pct * 0.8, CompatPhase::Transcoding),
                    {
                        let emit = emit.clone();
                        move |ok| {
                            if ok {
                                emit(100.0, CompatPhase::Completed);
                            } else {
                                emit(0.0, CompatPhase::Failed);
                            }
                        }
                    },
                    delivery,
                )
                .await
            } else {
                decoded
            }
        } else {
            decoded
        }
    };

    match &result {
        Ok(play) if play.progressive => emit(99.0, CompatPhase::Ready),
        Ok(_) => emit(100.0, CompatPhase::Completed),
        Err(_) => emit(0.0, CompatPhase::Failed),
    }
    let r = result?;
    Ok(CompatPlayResult {
        url: r.url,
        cached: r.cached,
        backend: r.backend,
        elapsed_ms: r.elapsed_ms,
        progressive: r.progressive,
    })
}

/// 离线 QA 固定缓存键：与前端面板 / 取消命令共用。
pub const QA_OFFLINE_LOCAL_SERIES: &str = "qa-offline-local";
pub const QA_OFFLINE_LOCAL_VID: u32 = 1;
const QA_OFFLINE_DEFAULT_REL: &str = "inputs/hg-bytevc2-plain.mp4";

/// 仅 `HONGGUO_OFFLINE_QA=1`：隔离目录内本地明文样本走兼容转码首段/MSE。
/// `file_path` 空则默认 `$HONGGUO_DATA_DIR/inputs/hg-bytevc2-plain.mp4`；路径必须落在隔离根内。
pub async fn qa_offline_local_play(
    state: &AppState,
    file_path: String,
) -> AppResult<CompatPlayResult> {
    if !hongguo_common::offline_qa::enabled() {
        return Err(AppError::Unsupported(
            "qa_offline_local_play 仅在 HONGGUO_OFFLINE_QA=1 时可用".into(),
        ));
    }
    let path = resolve_qa_offline_local_path(&file_path)?;
    let key = format!("{QA_OFFLINE_LOCAL_SERIES}:{QA_OFFLINE_LOCAL_VID}");
    let events = state.events().clone();
    let emit = {
        let key = key.clone();
        let events = events.clone();
        move |percent: f64, phase: CompatPhase| {
            events.publish(ServiceEvent::CompatProgress(CompatProgressEvent {
                key: key.clone(),
                percent,
                phase,
            }));
        }
    };
    let emit_tc = emit.clone();
    let result = ensure_inner(
        QA_OFFLINE_LOCAL_SERIES,
        QA_OFFLINE_LOCAL_VID,
        None,
        CompatSource::LocalFile(path),
        move |pct| emit_tc(pct, CompatPhase::Transcoding),
        {
            let emit = emit.clone();
            move |ok| {
                if ok {
                    emit(100.0, CompatPhase::Completed);
                } else {
                    emit(0.0, CompatPhase::Failed);
                }
            }
        },
        Delivery::TauriPrefix,
    )
    .await;
    match &result {
        Ok(play) if play.progressive => emit(99.0, CompatPhase::Ready),
        Ok(_) => emit(100.0, CompatPhase::Completed),
        Err(_) => emit(0.0, CompatPhase::Failed),
    }
    let r = result?;
    Ok(CompatPlayResult {
        url: r.url,
        cached: r.cached,
        backend: r.backend,
        elapsed_ms: r.elapsed_ms,
        progressive: r.progressive,
    })
}

fn resolve_qa_offline_local_path(file_path: &str) -> AppResult<PathBuf> {
    let raw = file_path.trim();
    let candidate = if raw.is_empty() {
        let root = hongguo_common::offline_qa::isolation_root().map_err(AppError::InvalidArgs)?;
        root.join(QA_OFFLINE_DEFAULT_REL)
    } else {
        let p = PathBuf::from(raw);
        if !p.is_absolute() {
            return Err(AppError::InvalidArgs(
                "离线 QA 本地样本路径必须是绝对路径".into(),
            ));
        }
        p
    };
    if hongguo_protocol::local::local_play_url(&candidate.to_string_lossy()).is_none() {
        return Err(AppError::InvalidArgs(
            "只允许播放视频文件（mp4/mkv/webm/mov/m4v）".into(),
        ));
    }
    let bound =
        hongguo_common::offline_qa::prepare_qa_io(&candidate).map_err(AppError::InvalidArgs)?;
    let meta = std::fs::metadata(&bound)
        .map_err(|e| AppError::NotFound(format!("本地样本不存在: {}: {e}", bound.display())))?;
    if !meta.is_file() {
        return Err(AppError::InvalidArgs(format!(
            "本地样本不是普通文件: {}",
            bound.display()
        )));
    }
    Ok(bound)
}

#[cfg(test)]
#[path = "compat_play/tests.rs"]
mod tests;
