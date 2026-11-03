//! 封面解码结果的内存缓存（gpui [`ImageCache`] 约定）：默认图片缓存进程退出
//! 即消失且无上限，这里给有上限的缓存。容量按张数——gpui 要求容量大于同时
//! 显示的图片数，否则可见图反复失缓存、逐帧重解码。逐出条目的纹理一律延迟
//! [`DROP_DELAY_MS`] 再丢：绘制中途丢纹理，本帧已排进的绘制指令会采样已销毁
//! 纹理（D3D/Metal 上表现为黑块）。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Entity, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource, Window,
};

/// 同时保留多少张解码后的封面（够任何一个列表页的整页）。
const CAPACITY: usize = 256;
/// 逐出缓冲：涨过 `CAPACITY + SLACK` 才一次性收缩回 [`CAPACITY`]——边界
/// 抖动会让本帧还可见的图反复失缓存、画面灰块闪烁。
const SLACK: usize = 64;
/// 逐出条目的纹理延迟丢弃时长（见模块注释「逐出必须躲开正在绘制的帧」）。
const DROP_DELAY_MS: u64 = 350;

pub(crate) struct CoverImageCache {
    /// 最近使用的在前：逐出从尾部取。
    items: VecDeque<(Resource, ImageCacheItem)>,
}

impl CoverImageCache {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        let cache = cx.new(|_| Self {
            items: VecDeque::new(),
        });
        // 缓存实体释放时把 GPU 纹理一并放掉（进程收尾路径，无需延迟）。
        cx.observe_release(&cache, |cache, cx| {
            for (_, item) in cache.items.drain(..) {
                if let Some(Ok(image)) = item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        cache
    }

    /// 逐出条目的纹理延迟丢弃：定时器回调在帧间执行（前台任务不与绘制
    /// 交错），那时引用这些纹理的帧早已展示完。
    fn drop_later(images: Vec<Arc<RenderImage>>, cx: &mut App) {
        if images.is_empty() {
            return;
        }
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(DROP_DELAY_MS))
                .await;
            cx.update(|cx| {
                for image in images {
                    cx.drop_image(image, None);
                }
            });
        })
        .detach();
    }
}

impl ImageCache for CoverImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let item = match self.items.iter().position(|(source, _)| source == resource) {
            Some(index) => self.items.remove(index).expect("下标在界内"),
            // miss 排查（RUST_LOG=debug）：正常会话一份封面只 miss 一次；
            // 同一张反复 miss = 解码-逐出循环，直接点名资源
            None => {
                log::debug!("[CoverCache] miss: {resource:?}");
                (resource.clone(), ImageCacheItem::new(resource, cx))
            }
        };
        self.items.push_front(item);
        if self.items.len() > CAPACITY + SLACK {
            let mut evicted = Vec::new();
            while self.items.len() > CAPACITY {
                if let Some((_, item)) = self.items.pop_back()
                    && let Some(Ok(image)) = item.get()
                {
                    evicted.push(image);
                }
            }
            Self::drop_later(evicted, cx);
        }
        self.items[0].1.use_image(window)
    }
}
