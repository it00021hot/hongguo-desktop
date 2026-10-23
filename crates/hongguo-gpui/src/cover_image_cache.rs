//! 封面解码结果的内存缓存：按 gpui 的 [`ImageCache`] 约定实现。
//!
//! **为什么要有它**：gpui 的默认图片缓存（App 资源缓存）按来源为键、**进程
//! 退出即消失**，而且没有上限——看过多少封面就留多少张解码结果。壳里给它一
//! 个有上限的缓存：解码结果只保留最近的一批，其余释放（GPU 纹理一并放掉）。
//!
//! **容量按张数而不是字节**：gpui 的图片缓存要求容量大于同时显示的图片数，
//! 否则可见的图会被逐出、每一帧都要重新解码。按张数取一个够列表页整页的
//! 值（卡片档约 287KB/张）。

use std::collections::VecDeque;
use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Entity, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource, Window,
};

/// 同时保留多少张解码后的封面（够任何一个列表页的整页）。
const CAPACITY: usize = 256;

pub(crate) struct CoverImageCache {
    /// 最近使用的在前：逐出从尾部取。
    items: VecDeque<(Resource, ImageCacheItem)>,
}

impl CoverImageCache {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        let cache = cx.new(|_| Self {
            items: VecDeque::new(),
        });
        // 缓存实体释放时把 GPU 纹理一并放掉。
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
            None => (resource.clone(), ImageCacheItem::new(resource, cx)),
        };
        self.items.push_front(item);
        while self.items.len() > CAPACITY {
            if let Some((_, evicted)) = self.items.pop_back()
                && let Some(Ok(image)) = evicted.get()
            {
                cx.drop_image(image, Some(window));
            }
        }
        self.items[0].1.use_image(window)
    }
}
