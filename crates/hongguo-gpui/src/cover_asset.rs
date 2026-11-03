//! 封面资产：远端封面 URL → 数据目录成品图，做成 gpui 的 [`Asset`]——去重/
//! 加载态/加载完重绘是框架的活，壳只留「下载 → HEIC 转码 → 缩到目标尺寸 →
//! 落盘」（`hongguo_protocol::cover`）。页面图片元素走
//! `img(ImageSource::Resource)`，解码、内存缓存与重绘归 gpui 图片缓存。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::{App, Asset};

/// 转码完成后的重绘合流窗口：一次首拉几十张，逐张全窗重绘会把主线程拆成
/// 碎帧——100ms 窗口内到齐的落盘合流为一次重绘。
const REFRESH_COALESCE_MS: u64 = 100;

/// 已有合流刷新在途（避免每次落盘都起一个定时任务）。
static REFRESH_QUEUED: AtomicBool = AtomicBool::new(false);

/// 请求一次合流的全窗重绘（见 [`REFRESH_COALESCE_MS`]）；唤醒检查与置位的
/// 次序保证窗口期内到达的落盘不丢请求。
fn schedule_refresh(cx: &mut App) {
    if REFRESH_QUEUED.swap(true, Ordering::SeqCst) {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(REFRESH_COALESCE_MS))
            .await;
        REFRESH_QUEUED.store(false, Ordering::SeqCst);
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// 远端封面 URL → 数据目录里的成品图就绪（按长边上限分档）。
/// 档位放在 Source 里（`gpui::Asset::load` 是静态方法，拿不到实例字段）。
pub(crate) struct CoverAsset;

impl Asset for CoverAsset {
    /// `(封面 URL, 目标长边)`：卡片 320 / 头像 64 / 全屏 1024。
    type Source = (String, u32);
    type Output = Result<(), String>;

    fn load(
        (source, edge): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        // gpui 的 `App::spawn` 只跑不返值，结果另走 oneshot 带回来。
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = cx.spawn(async move |cx: &mut gpui_kit::AsyncApp| {
            // 转码在壳的常驻 tokio 运行时 + 阻塞池里跑（内部要拉 ffmpeg 子
            // 进程），不占 GPUI 线程。
            let joined = crate::tokio()
                .spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        hongguo_protocol::cover::ensure_sized_cached(&source, edge)
                    })
                    .await
                    .map_err(|e| format!("封面转码任务失败: {e}"))?
                })
                .await;
            let result = match joined {
                Ok(result) => result.map(|_| ()),
                Err(error) => Err(format!("封面转码任务失败: {error}")),
            };
            // 成品图落盘后请求重绘（合流，见 [`schedule_refresh`]）——
            // `fetch_asset` 不订阅视图，只能全局刷新。
            cx.update(schedule_refresh);
            let _ = tx.send(result);
        });
        task.detach();
        async move {
            rx.await
                .unwrap_or_else(|_| Err("封面转码任务失败".to_string()))
        }
    }
}
