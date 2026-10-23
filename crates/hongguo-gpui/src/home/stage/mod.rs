//! 舞台渲染：tab 顶栏、沉浸舞台（封面占位层 / 视频槽二选一）、信息叠加层。
//! 画面由系统播放器的原生层（AVPlayerLayer / DComp 视频位）合成，GPUI 只
//! 在未出画时铺封面占位——本文件不写平台分支。

use gpui_kit::prelude::*;

use super::HomeView;
use super::feed::StreamItem;

/// 舞台周期状态日志（HONGGUO_DIAG 取证）的最小间隔（秒）。
const STAGE_DIAG_EVERY_SECS: u64 = 5;

// —— 渲染 ——
mod letterbox;
mod overlay;
mod render;

impl HomeView {
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
        if remote.is_empty() || self.stage_covers.contains_key(&sid) {
            return;
        }
        // 转码落盘（数据目录的原图档）后本地读回解码：转码一次、复用多次。
        let rx = crate::bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_protocol::cover::ensure_sized_cached(
                    &remote,
                    hongguo_protocol::cover::EDGE_FULL,
                )
            })
            .await
            .expect("阻塞池 join")
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let bytes = match result {
                Ok(path) => std::fs::read(path).ok(),
                Err(error) => {
                    log::warn!("[Home] 全屏占位封面转码失败: {error}");
                    None
                }
            };
            let image = bytes.as_deref().and_then(crate::cover_img::decode_full);
            let _ = this.update(cx, |view, cx| {
                if let Some(image) = image {
                    view.stage_covers.insert(sid, image);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
