//! 舞台渲染：tab 顶栏、沉浸舞台（封面占位层 / 视频槽二选一）、信息叠加层。
//! 画面由系统播放器的原生层（AVPlayerLayer / DComp 视频位）合成，GPUI 只
//! 在未出画时铺封面占位——本文件不写平台分支。

use std::sync::Arc;

use gpui_kit::RenderImage;
use gpui_kit::prelude::*;
use hongguo_common::AppError;

use super::HomeView;
use super::feed::StreamItem;

/// 舞台周期状态日志（HONGGUO_DIAG 取证）的最小间隔（秒）。
const STAGE_DIAG_EVERY_SECS: u64 = 5;

/// 舞台占位封面的常驻条数上限。每张是 1024 档原图（约 1MB），实际只用得上
/// 当前/目标这一部，攒着不释放只是白占内存。
const STAGE_COVER_LIMIT: usize = 8;

/// 一部剧的全屏占位封面所处的状态。
///
/// 三态各挡一件事：
/// - [`StageCover::Loading`]：请求已在途，**同剧不再重复发**。封面是异步
///   下载+转码+解码的，只看「已完成的表」的话，连点切集时每一集都会再发
///   一次 1024 档请求（实测「每集都获取封面」的根因）。
/// - [`StageCover::Ready`]：图已就绪，同剧换集直接复用。
/// - [`StageCover::Failed`]：这部的封面取不到，不再每次切集重试（1024 档的
///   下载+转码很贵，重试只会反复烧流量）。
pub(crate) enum StageCover {
    Loading,
    Ready(Arc<RenderImage>),
    Failed,
}

// —— 渲染 ——
mod letterbox;
mod overlay;
mod render;

impl HomeView {
    /// 舞台封面层当前该显示哪一部的封面：游标指向的那部（切剧瞬间游标已指向
    /// 下一部，先出它的封面）；游标条目没有封面条目时退回首帧方向已在播的
    /// 那部。加载与渲染共用这一个口径，避免两处各算一遍。
    pub(super) fn stage_cover_target(&self) -> Option<String> {
        self.display_items()
            .get(self.cursor())
            .map(|i| i.series_id.clone())
            .or_else(|| self.now.as_ref().map(|n| n.series_id.clone()))
    }

    /// 懒加载当前/目标剧的全屏占位封面（原图档）。
    ///
    /// **为什么这一处自己解**：gpui 的资产加载接口要 `&mut Window`，而舞台
    /// 渲染拿到的是 `&Window`；占位也只有当前这一集（图），不值得为它改
    /// 整条渲染链的借用。列表卡与头像走 gpui 的图片缓存（见
    /// [`crate::cover_img`]）。
    pub(super) fn load_cover_of(&mut self, item: &StreamItem, cx: &mut Context<Self>) {
        let remote = if item.horiz_cover.is_empty() {
            item.cover.clone()
        } else {
            item.horiz_cover.clone()
        };
        let sid = item.series_id.clone();
        if remote.is_empty() {
            return;
        }
        // 在途 / 已就绪 / 已失败：这一部都不再发请求（见 [`StageCover`]）。
        if self.stage_covers.contains_key(&sid) {
            return;
        }
        self.stage_covers.insert(sid.clone(), StageCover::Loading);
        // 转码落盘（数据目录的原图档）后本地读回解码：转码一次、复用多次。
        // 读盘 + JPEG 解码 + 缩放都留在阻塞池——cx.spawn 跑在 GPUI 前台执行
        // 器（主线程）上，1024px 档的解码搬上来就是每次上滑一记主线程长停顿。
        let rx = crate::bridge::spawn(async move {
            let image = tokio::task::spawn_blocking(move || {
                hongguo_protocol::cover::ensure_sized_cached(
                    &remote,
                    hongguo_protocol::cover::EDGE_FULL,
                )
                .map_err(|error| {
                    log::warn!("[Home] 全屏占位封面转码失败: {error}");
                    error
                })
                .ok()
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| crate::cover_img::decode_full(&bytes))
            })
            .await
            .map_err(|e| AppError::Internal(format!("全屏占位封面阻塞池任务失败: {e}")))?;
            Ok::<_, AppError>(image)
        });
        cx.spawn(async move |this, cx| {
            let image = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                let state = match image {
                    Ok(Some(image)) => StageCover::Ready(image),
                    Ok(None) => StageCover::Failed,
                    Err(e) => {
                        log::warn!("[Home] 全屏占位封面读取失败: {e}");
                        StageCover::Failed
                    }
                };
                view.stage_covers.insert(sid.clone(), state);
                view.evict_stage_covers();
                cx.notify();
            });
        })
        .detach();
    }

    /// 占位封面超出条数上限时逐出一部（**不动当前正在显示的那张**）。
    fn evict_stage_covers(&mut self) {
        if self.stage_covers.len() <= STAGE_COVER_LIMIT {
            return;
        }
        let target = self.stage_cover_target();
        let victim = self
            .stage_covers
            .keys()
            .find(|sid| Some(sid.as_str()) != target.as_deref())
            .cloned();
        if let Some(victim) = victim {
            self.stage_covers.remove(&victim);
        }
    }
}
