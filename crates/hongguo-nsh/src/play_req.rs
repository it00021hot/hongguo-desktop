//! 跨页播放请求：历史 / 收藏 / 追更页的「继续播放」都落在首页那一路
//! `video` 上（播放不是独立页面）。请求经这份壳级信号中转——列表页
//! `request(...)` 后切回首页，播放页的 effect 取走并按（剧, 集号, 起始
//! 秒）起播；播放页没挂载时请求就在信号里等着，挂载即消费。

use nsh_core::ui::prelude::*;

use std::sync::LazyLock;

/// 一次播放请求：目标剧 + 1 基集号 + 起始秒（0 = 从头）。
#[derive(Debug, Clone)]
pub struct PlayRequest {
    pub series_id: String,
    pub vid_index: u32,
    pub start_secs: f64,
}

static PENDING: LazyLock<Signal<Option<PlayRequest>>> =
    LazyLock::new(|| signal_with(None, SignalEquals::Never));

/// 组件体里触碰一次：信号节点跟着组件树建起来（首个使用点可能在更深
/// 的页面组件里，那里不再是组件体）。
pub fn init() {
    LazyLock::force(&PENDING);
}

/// 发一个播放请求（列表页的「继续播放」入口）。
pub fn request(series_id: String, vid_index: u32, start_secs: f64) {
    log::info!("[PlayReq] series={series_id} 集={vid_index} 位置={start_secs:.1}s");
    PENDING.set(Some(PlayRequest {
        series_id,
        vid_index,
        start_secs,
    }));
}

/// 播放页取走挂起的请求（有就消费清空）。
pub fn take() -> Option<PlayRequest> {
    let pending = PENDING.with(|v| v.clone());
    if pending.is_some() {
        PENDING.set(None);
    }
    pending
}
