//! 封面解码结果的内存缓存：按 gpui 的 [`ImageCache`] 约定实现。
//!
//! **为什么要有它**：gpui 的默认图片缓存（App 资源缓存）按来源为键、**进程
//! 退出即消失**，而且没有上限——看过多少封面就留多少张解码结果。壳里给它一
//! 个有上限的缓存：解码结果只保留最近的一批，其余释放（GPU 纹理一并放掉）。
//!
//! **容量按张数而不是字节**：gpui 的图片缓存要求容量大于同时显示的图片数，
//! 否则可见的图会被逐出、每一帧都要重新解码。按张数取一个够列表页整页的
//! 值（卡片档约 550KB/张解码位图 + 同量级 GPU 纹理，满载约 350MB 内存，
//! 常态远低于此）。
//!
//! **逐出必须躲开「正在绘制的帧」**：纹理只在帧与帧之间丢才安全——某帧绘制
//! 中途丢纹理，本帧已排进场景的绘制指令就会采样已销毁的纹理（D3D/Metal 上
//! 表现为黑块），这就是「出了图又变黑」的来历。逐出条目的纹理一律延迟
//! [`DROP_DELAY_MS`] 再丢，那时引用它的帧早已展示完。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Entity, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource, Window,
};

/// 同时保留多少张解码后的封面（够任何一个列表页的整页）。
const CAPACITY: usize = 256;
/// 逐出缓冲：涨过 `CAPACITY + SLACK` 才一次性收缩回 [`CAPACITY`]。
///
/// 一帧内同时显示的图片数在 `CAPACITY` 附近时（历史页长列表会涨到几百
/// 行），若涨一点就逐出，被逐出的恰是本帧还可见的图——它们反复失缓存、
/// 反复解码，画面出现灰块闪烁。SLACK 把这类边界抖动吸收掉。
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
