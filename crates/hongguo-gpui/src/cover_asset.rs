//! 封面资产：把「远端封面 URL → 数据目录里的成品图」做成 gpui 的 [`Asset`]。
//!
//! **为什么交给 gpui**：加载这一段（去重、加载态、加载完成后重绘当前视图）
//! 本来就是框架的活——自造一套「自己解码 + 自己记在途 + 自己 notify 重绘」
//! 是重复造轮子。壳这边只留业务必需的那一步：下载 → HEIC 转码 → 缩到目标
//! 尺寸 → 落盘到数据目录（`hongguo_protocol::cover`）。
//!
//! 页面拿到的图片元素走 `img(ImageSource::Resource(Resource::Path(..)))`：
//! 解码、内存缓存、GPU 纹理与重绘都归 gpui 的图片缓存管。

use gpui_kit::{App, Asset};

/// 远端封面 URL → 数据目录里的成品图就绪（按长边上限分档）。
///
/// 档位放在 Source 里（`gpui::Asset::load` 是静态方法，拿不到实例字段）：
/// `(封面 URL, 目标长边)`。
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
            // 成品图落盘后刷新窗口：等这张图的视图重绘（`fetch_asset` 不订阅
            // 视图，只能全局刷新）。
            cx.refresh();
            let _ = tx.send(result);
        });
        task.detach();
        async move {
            rx.await
                .unwrap_or_else(|_| Err("封面转码任务失败".to_string()))
        }
    }
}
