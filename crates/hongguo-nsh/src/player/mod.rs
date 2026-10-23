//! 播放域：首页 = 沉浸式播放（对齐 Tauri 壳首页 `player-page.tsx` 的形态）。
//!
//! 视频层占满整个内容区（大窗 contain / 小窗 cover），悬浮层按 Tauri 的
//! 位置叠在画面上：左下信息层、右侧互动栏、底部控制栏、右侧评论面板、
//! 顶部加载细条与错误条。子模块按域拆：`actions`（编排动作）/ `stage`
//! （舞台状态束）/ `view`（舞台视图组合）/ `feed`（取流与接口封装）/
//! `controls` / `mini` / `picker` / `rail` / `info` / `danmaku` / `comments`
//! / `widgets` / `icons`。
//!
//! 交互语义对齐 Tauri：单击画面 = 播放/暂停（并唤醒悬浮层）；悬浮层
//! 静止 3 秒自动收（暂停与任一面板打开时常显）；↑↓ 未连播 = 换剧、
//! 点「观看完整短剧」或自动连播完成本集后 = 切集（Esc 退出连播）；
//! 空格播放暂停、←→ 快退快进 5 秒。滚轮切换 nsh 0.1.1 没有滚轮事件，
//! 暂缺（升级后补）。

pub(crate) mod actions;
pub(crate) mod cadence;
pub(crate) mod comments;
pub(crate) mod comments_view;
pub(crate) mod controls;
pub(crate) mod danmaku;
pub(crate) mod download;
pub(crate) mod feed;
pub(crate) mod history;
pub(crate) mod icons;
pub(crate) mod info;
pub(crate) mod mini;
pub(crate) mod picker;
pub(crate) mod rail;
pub(crate) mod stage;
pub(crate) mod view;
pub(crate) mod widgets;

use nsh_core::ui::prelude::*;

use crate::nav::FeedNav;

// 编排动作收口：子模块（controls / mini）继续走 `super::动作`；view 直接
// 从 actions 引（长按倍速与 on_end 只在视频层用）。
pub(crate) use actions::{
    fetch_feed, load_related, pick_episode, play_request, replay_episode, seek_ratio, seek_to,
    send_danmaku, step_chrome, step_episode, switch_series, toggle_frameless, toggle_fullscreen,
    toggle_incognito, toggle_portrait_full, wake,
};
pub(crate) use stage::Stage;

/// 推荐流首个 tab（抓包锁定：推荐 16 / 漫剧 36 / 真人剧 39）。
pub(crate) const TAB_FEED: &str = "16";
/// 顶部分类 tab（服务端顶栏下发前/失败时的回落三件套）。
pub(crate) const TABS: [(&str, &str); 3] = [
    ("16", "home.tabFeed"),
    ("36", "home.tabComic"),
    ("39", "home.tabHuman"),
];
/// 倍速档位（与主流播放器一致，Tauri 同表）。
pub(crate) const RATES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
/// 悬浮层静止多久后收起（B 站方案，Tauri 同款 3 秒）。
const CHROME_HIDE_MS: u64 = 3_000;
/// 光标轮询的「动了」判定阈值（点）：低于它算手抖/采样噪声，不唤醒。
const CURSOR_MOVE_EPS: f32 = 2.0;
/// 纵向滑动切剧的累计位移阈值（点）：低于它算误触，不切。
pub(crate) const PAN_SWITCH_MIN: f32 = 40.0;

/// 首页（沉浸式播放）。
#[component]
pub fn PlayerPage() -> impl IntoElement {
    let feed = FeedNav::from_context();
    let s = Stage::new();

    // ---- 顶栏 tab 一变就整源重开（Tauri 的「切 tab 即换片」语义）----
    {
        let s = s.clone();
        let feed_tab = feed.current();
        effect(move || {
            let tab = feed_tab.get();
            batch(|| {
                s.items.set(Vec::new());
                s.session.replace(String::new());
                s.next_offset.replace(0);
                s.has_more.replace(false);
                s.index.set(0);
                s.binge.set(false);
            });
            fetch_feed(&s, &tab, false);
        });
    }

    // ---- 当前集一变：右栏计数 / 左下角标 / 组合键跟上（档案公开计数）----
    {
        let s = s.clone();
        effect(move || {
            let index = s.current_index.get();
            let episode = s
                .episodes
                .with(|list| list.iter().find(|e| e.vid_index == index).cloned());
            if let Some(episode) = episode {
                s.rail.comment_count.set(episode.comment_count);
                s.rail.digg_count.set(episode.digg_count);
                s.rail_ctx.bare_vid.set(episode.vid);
            }
            s.info.vid_index.set(index);
        });
    }

    // ---- 评论区开合：互动栏 💬 触发打开（整页重拉）；面板/键盘关闭后
    //      同步清掉触发位，便于再次打开 ----
    {
        let s = s.clone();
        effect(move || {
            if s.rail_ctx.open_comments.get() {
                s.comments.open(s.rail_ctx.bare_vid, s.series_id);
            }
        });
    }
    {
        let s = s.clone();
        effect(move || {
            if !s.comments.open.get() && s.rail_ctx.open_comments.get() {
                s.rail_ctx.open_comments.set(false);
            }
        });
    }

    // ---- 观看历史上报节拍 / 播放进度落盘（1 秒一拍；暂停沿 force 收尾）----
    history::spawn_heartbeat(s.clone());

    // ---- 外部播放请求（历史/收藏/追更的「继续播放」）：取走并按
    //      （剧, 集号, 起始秒）起播 ----
    {
        let s = s.clone();
        effect(move || {
            if let Some(req) = crate::play_req::take() {
                play_request(&s, req);
            }
        });
    }

    // ---- 悬浮层自动收：播放中、无面板、静止超 3 秒 ----
    {
        let s = s.clone();
        spawn(async move {
            loop {
                timer(std::time::Duration::from_millis(500)).await;
                if !s.playing.get() || s.panel_open() || !s.chrome_shown.get() {
                    continue;
                }
                let idle = s.activity.with(|at| at.elapsed());
                if idle >= std::time::Duration::from_millis(CHROME_HIDE_MS) {
                    s.chrome_shown.set(false);
                    s.close_popups();
                }
            }
        });
    }

    // ---- 鼠标移动唤醒 + 隐身模式：nsh 0.1.1 没有指针移动事件，150ms
    //      轮询光标位置。移动唤醒：在窗口内动了（含刚进窗口）就唤醒——
    //      Tauri mousemove/mouseenter 的同语义替代。隐身（incognito.ts
    //      同款）：开着时离窗 → 整窗透明 + 暂停；回窗 → 重现 + 只续被隐身
    //      欠下的那次播放（用户自己按停的不欠）。 ----
    {
        let s = s.clone();
        let last = stored(None::<(f32, f32)>);
        let was_hidden = stored(false);
        let owed_play = stored(false);
        spawn(async move {
            loop {
                timer(std::time::Duration::from_millis(150)).await;
                let point = crate::platform::cursor_in_window();
                // 隐身分支先行：开着时光标离窗就藏，回窗就还。
                if s.incognito.get() {
                    match point {
                        Some(_) => {
                            if was_hidden.replace(false) {
                                crate::platform::set_window_alpha(1.0);
                                if owed_play.replace(false) {
                                    s.playing.set(true);
                                }
                            }
                        }
                        None => {
                            if !was_hidden.with(|v| *v) {
                                was_hidden.replace(true);
                                // 只在「正在播」时欠：已经暂停着是用户按的，不欠
                                owed_play.replace(s.playing.get());
                                if s.playing.get() {
                                    s.playing.set(false);
                                }
                                crate::platform::set_window_alpha(0.05);
                            }
                        }
                    }
                } else if was_hidden.replace(false) {
                    // 关掉隐身开关与离窗回窗同一条恢复路径。
                    crate::platform::set_window_alpha(1.0);
                    owed_play.replace(false);
                }

                let Some(point) = point else {
                    last.replace(None);
                    continue;
                };
                let moved = last.with(|prev| match *prev {
                    Some(prev) => {
                        (prev.0 - point.0).abs() > CURSOR_MOVE_EPS
                            || (prev.1 - point.1).abs() > CURSOR_MOVE_EPS
                    }
                    None => true,
                });
                last.replace(Some(point));
                if moved {
                    wake(&s);
                }
            }
        });
    }

    // ---- 键盘（nsh 的 When::Auto 保证输入框打字不触发；Esc 不受影响）----
    {
        let s = s.clone();
        on_key("space", move |_| {
            wake(&s);
            s.playing.update(|p| *p = !*p);
        });
    }
    {
        let s = s.clone();
        // 按住 ←→ 连续快退/快进（浏览器 keydown 自然重复，Tauri 同款；
        // nsh 默认按住只发一次，repeat: true 打开连发）。space/↑↓ 不开——
        // 连发会疯狂切播/连跳剧。
        on_key_with(
            "left",
            KeyOptions {
                repeat: true,
                ..Default::default()
            },
            move |_| {
                wake(&s);
                seek_to(&s, (s.time.get() - 5.0).max(0.0));
            },
        );
    }
    {
        let s = s.clone();
        on_key_with(
            "right",
            KeyOptions {
                repeat: true,
                ..Default::default()
            },
            move |_| {
                wake(&s);
                seek_to(&s, s.time.get() + 5.0);
            },
        );
    }
    {
        let s = s.clone();
        on_key("up", move |_| step_chrome(&s, -1));
    }
    {
        let s = s.clone();
        on_key("down", move |_| step_chrome(&s, 1));
    }
    {
        let s = s.clone();
        on_key("escape", move |_| {
            // 链序对齐 Tauri（use-player-interactions Esc）：竖屏铺满最先，
            // 然后面板（评论/选集/弹层），无边框，连播，最后紧凑形态本身。
            if s.comments.open.get() {
                s.comments.open.set(false);
                return;
            }
            if s.panel_open() {
                s.picker_open.set(false);
                s.close_popups();
                return;
            }
            if s.portrait.get() && s.portrait_full.get() {
                s.toggle_portrait_full(); // 铺满回竖条（hgplayer 同序）
                return;
            }
            if s.frameless.get() {
                s.frameless.set(false);
                return;
            }
            if s.binge.get() {
                s.binge.set(false);
                return;
            }
            if s.mini.get() {
                s.mini.set(false);
                return;
            }
            if s.portrait.get() {
                s.exit_portrait();
            }
        });
    }

    // 舞台整体交给 stage_view（收 owned：Rust 2024 的 RPIT 会捕获引用
    // 生命周期，传引用会让返回值借不到 'static）。
    view::stage_view(s)
}
