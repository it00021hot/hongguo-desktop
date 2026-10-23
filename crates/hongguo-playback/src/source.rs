//! 流的样本级读取：按样本表从 `serve()` 取明文字节。
//!
//! serve 是同步阻塞入口（未就绪的区间在条件变量上等，读者需求会驱动填充
//! 优先供这段），一律放 tokio 阻塞池。顺序读是常态，用「1MB 窗口」缓存摊薄
//! 调用次数：视频 1MB 够 1-2s，音频 1MB 够一分钟量级。

use std::sync::Arc;
use std::sync::Mutex;

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform::platform::video::VideoCodec;
use hongguo_video::sample_table::TrackInfo;

/// 单次 serve 拉取的窗口大小（对齐填充的分块口径）。
const WINDOW: u64 = 1024 * 1024;

/// 头部读取的窗口：moov 解析在累积头上反复重跑，窗口越小串行 RTT 越多
/// ——「头部就绪 200-260ms」的主体就是那 2-4 个串行往返，4MB 一窗通常
/// 1-2 轮就齐。样本读取仍用 1MB（顺序消费，大窗只多占内存）。
const HEAD_WINDOW: u64 = 4 * 1024 * 1024;

/// 播放数据源：在线流（`(vid, 档位)` 走 `hongguo-stream` 缓存）或本地成品
/// 文件。
///
/// 本地成品 = 「已下载的那一集」（Tauri `hongguo-local://` 的同位形态）：
/// 字节从磁盘来、无档位概念、离线可播。引擎只认「按样本表取字节」这件事，
/// 两种源在字节层同构——所以源抽象放在这一层，上面（样本表 / 解码 / 时钟）
/// 一行不用改。
#[derive(Clone)]
pub(crate) enum ByteSource {
    Online {
        vid: String,
        definition: u32,
    },
    Local {
        /// 已打开的成品文件 + 顺序读保护（样本按序读，偶发乱序靠锁串行化）。
        file: Arc<Mutex<std::fs::File>>,
        len: u64,
    },
}

impl ByteSource {
    /// 打开本地成品。失败（文件不在/没权限）由调用方回落在在线流。
    pub(crate) fn local(path: &std::path::Path) -> AppResult<Self> {
        let file = std::fs::File::open(path)
            .map_err(|e| AppError::Media(format!("本地成品打不开（{}）: {e}", path.display())))?;
        let len = file
            .metadata()
            .map_err(|e| AppError::Media(format!("本地成品读不到长度: {e}")))?
            .len();
        Ok(Self::Local {
            file: Arc::new(Mutex::new(file)),
            len,
        })
    }

    /// 源总长（在线：`bytes=0-0` 的 Content-Range；本地：文件长度）。
    pub(crate) async fn total(&self) -> AppResult<u64> {
        match self {
            Self::Online { vid, definition } => probe_total(vid, *definition).await,
            Self::Local { len, .. } => Ok(*len),
        }
    }

    /// 取一段明文字节（闭区间）。本地读走阻塞池；`cancelled` 只在等待期间
    /// 检查——本地盘读很快，不需要 serve 那套读者登记。
    pub(crate) async fn read(
        &self,
        start: u64,
        end: u64,
        cancelled: impl Fn() -> bool + Send + Sync + 'static,
    ) -> AppResult<Vec<u8>> {
        match self {
            Self::Online { vid, definition } => {
                read_range(vid.clone(), *definition, start, end, cancelled).await
            }
            Self::Local { file, .. } => {
                let file = Arc::clone(file);
                tokio::task::spawn_blocking(move || {
                    if cancelled() {
                        return Ok(Vec::new());
                    }
                    let mut f = file.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    use std::io::{Read, Seek, SeekFrom};
                    f.seek(SeekFrom::Start(start))
                        .map_err(|e| AppError::Media(format!("本地成品定位失败: {e}")))?;
                    let want = (end.saturating_sub(start) + 1) as usize;
                    let mut buf = vec![0u8; want];
                    let mut read = 0usize;
                    while read < want {
                        let n = f
                            .read(&mut buf[read..])
                            .map_err(|e| AppError::Media(format!("本地成品读取失败: {e}")))?;
                        if n == 0 {
                            break;
                        }
                        read += n;
                    }
                    buf.truncate(read);
                    Ok(buf)
                })
                .await
                .map_err(|e| AppError::Internal(format!("阻塞池 join: {e}")))?
            }
        }
    }
}

/// 流的静态信息（moov 解析产物）。
pub(crate) struct StreamMeta {
    pub video: TrackInfo,
    pub audio: Option<TrackInfo>,
    /// 视频轨编码（选解码会话与序列头形态用）。
    pub video_codec: VideoCodec,
    pub video_parameter_sets: Vec<Vec<u8>>,
    /// 序列头原始载荷（HEVC 的 hvcC / H.264 的 avcC；Windows MF 的形态，
    /// macOS 不用）。
    pub video_sequence_header: Option<Vec<u8>>,
    /// 样本 NAL 长度前缀字节数（带内参数集前插用）。
    pub video_length_size: usize,
}

/// 从缓存流里读头部直到样本表完整（moov 落在头部，渐进注册已保证就绪）。
pub(crate) async fn open_stream(source: &ByteSource) -> AppResult<StreamMeta> {
    let total = source.total().await?;
    let mut head: Vec<u8> = Vec::new();
    let mut cursor = 0u64;
    for _ in 0..16 {
        // 64MB 封顶：moov 不可能这么大，超了说明流不对
        if cursor >= total {
            break;
        }
        let want = HEAD_WINDOW.min(total - cursor);
        let chunk = source.read(cursor, cursor + want - 1, || false).await?;
        if chunk.is_empty() {
            break;
        }
        cursor += chunk.len() as u64;
        head.extend_from_slice(&chunk);
        if let Ok(demuxed) = hongguo_media::demux::demux_bytes(&head)
            && let Some(video) = demuxed.video_track()
        {
            // 序列头原始载荷尽力而为：Windows MF 走它做序列头，取不到时
            // 解码端有带内参数集兜底
            let (codec, (nalus, length_size), sequence_header) =
                if hongguo_media::hevc::is_hevc(&video.info.codec) {
                    let (nalus, length_size) =
                        hongguo_media::hevc::param_nalus_from_head(&head, &video.info)?;
                    let hvcc = hongguo_media::hevc::hvcc_payload_from_head(&head, &video.info)
                        .map_err(|e| {
                            log::debug!("[Playback] hvcC 载荷取不到（走带内兜底）: {e}");
                            e
                        })
                        .ok();
                    (VideoCodec::Hevc, (nalus, length_size), hvcc)
                } else if hongguo_media::h264::is_h264(&video.info.codec) {
                    let (nalus, length_size) =
                        hongguo_media::h264::param_nalus_from_head(&head, &video.info)?;
                    let avcc = hongguo_media::h264::avcc_payload_from_head(&head, &video.info)
                        .map_err(|e| {
                            log::debug!("[Playback] avcC 载荷取不到（走带内兜底）: {e}");
                            e
                        })
                        .ok();
                    (VideoCodec::H264, (nalus, length_size), avcc)
                } else {
                    return Err(AppError::Media(format!(
                        "播放引擎只接 HEVC / H.264 轨（codec = {}）",
                        video.info.codec
                    )));
                };
            return Ok(StreamMeta {
                audio: demuxed.audio_track().map(|t| t.info.clone()),
                video_codec: codec,
                video_parameter_sets: nalus,
                video_sequence_header: sequence_header,
                video_length_size: length_size,
                video: video.info.clone(),
            });
        }
    }
    Err(AppError::Media("头部拿不到完整样本表（moov）".into()))
}

/// 文件总长：`bytes=0-0` 的 Content-Range 尾段。
async fn probe_total(vid: &str, definition: u32) -> AppResult<u64> {
    let (_, headers, _) = read_range_raw(vid.to_string(), definition, 0, 0, || false).await?;
    let cr = headers
        .iter()
        .find(|(k, _)| k == "Content-Range")
        .map(|(_, v)| v.clone())
        .ok_or_else(|| AppError::Media("取流响应没有 Content-Range".into()))?;
    cr.rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| AppError::Media(format!("Content-Range 无法解析: {cr}")))
}

/// 读样本 idx 的字节（窗口缓存：顺序读命中，乱序读自动重灌）。
pub(crate) struct SampleSource {
    source: ByteSource,
    samples: Arc<Vec<(u64, u64)>>,
    buf: Vec<u8>,
    buf_at: u64,
}

impl SampleSource {
    pub(crate) fn new(source: ByteSource, samples: Arc<Vec<(u64, u64)>>) -> Self {
        Self {
            source,
            samples,
            buf: Vec::new(),
            buf_at: u64::MAX,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }

    /// 读样本 idx 的字节；`cancelled` 命中时，等待中的 serve 会被尽快唤醒
    /// 退场（注销读者登记），本调用以错误返回。
    ///
    /// seek 之后还堵在旧位置上的读取必须主动让路——干等旧数据的话，填充
    /// 调度器会先满足旧需求（FIFO），新位置的 seek 就绪被白白拖后一个区间。
    pub(crate) async fn read_cancellable(
        &mut self,
        idx: usize,
        cancelled: impl Fn() -> bool + Send + Sync + 'static,
    ) -> AppResult<Vec<u8>> {
        self.read_inner(idx, cancelled).await
    }

    async fn read_inner(
        &mut self,
        idx: usize,
        cancelled: impl Fn() -> bool + Send + Sync + 'static,
    ) -> AppResult<Vec<u8>> {
        let (offset, size) = *self
            .samples
            .get(idx)
            .ok_or_else(|| AppError::Media(format!("样本号越界: {idx}")))?;
        if size == 0 {
            return Ok(Vec::new());
        }
        let end = offset + size; // 开区间
        if !(offset >= self.buf_at && end <= self.buf_at + self.buf.len() as u64) {
            self.buf_at = offset;
            self.buf = self
                .source
                .read(offset, offset + WINDOW - 1, cancelled)
                .await?;
        }
        let start = (offset - self.buf_at) as usize;
        self.buf
            .get(start..start + size as usize)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| AppError::Media(format!("样本 {idx} 越出读窗口")))
    }
}

/// 取一段明文字节（serve 阻塞，放阻塞池；`cancelled` 透传给 serve，
/// seek 重定位时等待中的请求能尽快退场并注销读者登记）。
async fn read_range(
    vid: String,
    definition: u32,
    start: u64,
    end: u64,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> AppResult<Vec<u8>> {
    let (status, _, body) = read_range_raw(vid, definition, start, end, cancelled).await?;
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        Err(AppError::Media(format!(
            "取流区间 {start}-{end} 失败（HTTP {status}）"
        )))
    }
}

async fn read_range_raw(
    vid: String,
    definition: u32,
    start: u64,
    end: u64,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> AppResult<hongguo_stream::range::ProtocolResponse> {
    let header = format!("bytes={start}-{end}");
    tokio::task::spawn_blocking(move || {
        hongguo_stream::serve_cancellable(&vid, definition, Some(&header), cancelled)
            .map_err(|e| AppError::Media(format!("取流失败: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("阻塞池 join: {e}")))?
}
