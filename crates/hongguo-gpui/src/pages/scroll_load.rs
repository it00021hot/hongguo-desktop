//! 列表滚动哨兵：Tauri 各页 `IntersectionObserver` 哨兵的 GPUI 等价物。
//!
//! 用法（每个列表页一份）：
//! 1. 视图里持一个 [`ScrollLoad`]；
//! 2. 滚动容器挂 `.track_scroll(scroll_load.handle())`；
//! 3. `render` 开头 `if self.scroll_load.should_load(has_more, loading) { self.load_more(cx) }`。
//!
//! 判定基于 [`ScrollHandle`] 的 offset/max_offset：距底 < [`TRIGGER`] 且当前
//! 允许加载时报一次；滚离底部超过 [`REARM`]（或这一页拉完、`has_more`
//! 由假转真）后重新武装——一次触底只发一页请求，不会连环翻页。
//! 各页原有的「加载更多」按钮保留作键盘/无障碍兜底。

use gpui_kit::ScrollHandle;

/// 距底多少像素内触发（约两行卡片的高度，滚动手感上像「还没到底就开始拉」）。
const TRIGGER: f32 = 240.0;
/// 离开底部多少像素后重新武装（比 TRIGGER 大一截，防抖防止在边界反复触发）。
const REARM: f32 = 640.0;

pub(crate) struct ScrollLoad {
    handle: ScrollHandle,
    /// 可以触发下一次加载（触底触发后置假，离开底部再置真）。
    armed: bool,
}

impl ScrollLoad {
    pub(crate) fn new() -> Self {
        Self {
            handle: ScrollHandle::new(),
            armed: true,
        }
    }

    /// 挂到滚动容器：`.track_scroll(scroll_load.handle())`。
    pub(crate) fn handle(&self) -> &ScrollHandle {
        &self.handle
    }

    /// 每帧（render 开头）判一次：返回真 = 该续页了，调用方跑本页的
    /// `load_more`。`loading` 为真（上一页在途）时不触发，也不消耗武装态。
    pub(crate) fn should_load(&mut self, has_more: bool, loading: bool) -> bool {
        if !has_more {
            // 没有下一页：保持武装，切 tab / 换筛选后重新到底能立刻触发
            self.armed = true;
            return false;
        }
        let offset: f32 = self.handle.offset().y.into();
        let max: f32 = self.handle.max_offset().y.into();
        // max == 0：首帧还没完成布局（或内容根本没超过视口）。首帧不能当成
        // 「已到底」——否则每次进页面都会白拉一页（实测启动连拉两页）。
        if max <= 0.0 {
            return false;
        }
        let distance = max - offset;
        if distance > REARM {
            self.armed = true;
            return false;
        }
        if self.armed && !loading && distance <= TRIGGER {
            self.armed = false;
            return true;
        }
        false
    }
}
