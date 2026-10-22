//! 播放页编排动作：换剧/切集/seek/发弹幕/档案装载/推荐流分页。
//!
//! 全部是 [`Stage`] 上的自由函数（壳不复制业务——每个动作对应
//! service/api 的同一批调用），控制栏、小窗条与键盘接线都从 `super` 取用。
//! `pub(crate) use` 在 `mod.rs` 收口，子模块继续走 `super::动作`。

use nsh_core::ui::prelude::*;

use hongguo_api::detail::RelatedItem;
use hongguo_common::model::Episode;

use crate::i18n::t;
use crate::nav::FeedNav;

use super::{RATES, Stage, danmaku, feed, rail};

// ---------------------------------------------------------------- 编排动作

/// 「用户在场」：重置 3 秒倒计时并亮出悬浮层。
pub(crate) fn wake(s: &Stage) {
    s.activity.replace(std::time::Instant::now());
    s.chrome_shown.set(true);
}

/// 键盘 ↑↓：连播中切集，否则换剧（沉浸流语义）。
pub(crate) fn step_chrome(s: &Stage, delta: i32) {
    wake(s);
    if s.binge.get() {
        step_episode(s, delta);
    } else {
        step_series(s, delta);
    }
}

/// 长按快进的倍速（hgplayer/TikTok 同款按住看）。
const LONG_PRESS_RATE: f32 = 2.0;

/// 长按开始：记下原档位并切到长按倍速（暂停中没意义，不接管）。
pub(crate) fn long_press_rate_on(s: &Stage) {
    if !s.playing.get() {
        return;
    }
    let current = RATES[s.rate_index.get().min(RATES.len() - 1)];
    if (current - LONG_PRESS_RATE).abs() < f32::EPSILON {
        return;
    }
    s.long_press_rate.replace(Some(current));
    if let Some(at) = RATES
        .iter()
        .position(|r| (*r - LONG_PRESS_RATE).abs() < f32::EPSILON)
    {
        s.rate_index.set(at);
    }
    toast(
        format!("{} {LONG_PRESS_RATE}x", t("player.playbackRate")),
        ToastDuration::Short,
    );
}

/// 长按结束（抬起/滑出）：恢复原档位。
pub(crate) fn long_press_rate_off(s: &Stage) {
    let Some(rate) = s.long_press_rate.replace(None) else {
        return;
    };
    if let Some(at) = RATES.iter().position(|r| (*r - rate).abs() < f32::EPSILON) {
        s.rate_index.set(at);
    }
}

/// 全屏开关（镜像态同步给图标用）。
pub(crate) fn toggle_fullscreen(s: &Stage) {
    crate::platform::toggle_fullscreen();
    s.fullscreen.set(crate::platform::is_fullscreen());
}

/// 无边框播放开关（App chrome 藏显由 app.rs 按同一信号裁决）。
pub(crate) fn toggle_frameless(s: &Stage) {
    let next = !s.frameless.get();
    s.frameless.set(next);
}

/// 隐身模式开关（离窗透明+暂停、回窗续播的判定在播放域轮询任务里）。
pub(crate) fn toggle_incognito(s: &Stage) {
    let next = !s.incognito.get();
    s.incognito.set(next);
}

/// 竖屏铺满档切换（竖屏中全屏语义让位给铺满；非竖屏 no-op）。
pub(crate) fn toggle_portrait_full(s: &Stage) {
    s.toggle_portrait_full();
}

/// 换一部剧（沉浸流步进）：先用流条目自带的 vid 起播（秒开），
/// 再拉分集表把选集/自动连播接上。
fn play_series_at(s: &Stage, at: usize) {
    let Some(entry) = s.items.with(|list| list.get(at).cloned()) else {
        return;
    };
    batch(|| {
        s.index.set(at);
        s.episodes.set(Vec::new());
        s.current_index.set(0);
        s.related.set(Vec::new());
        s.binge.set(false);
        reset_media(s);
        s.info.title.set(entry.title.clone());
        s.info.heat_text.set(entry.heat_text.clone());
        s.info.season_tag.set(entry.season_tag.clone());
        s.info.badge.set(entry.badge.clone());
        s.info.intro.set(String::new());
        s.info.episode_count.set(entry.episode_cnt);
    });
    s.rail_ctx.title.set(entry.title.clone());
    s.rail_ctx.series_id.set(entry.series_id.clone());
    s.backdrop.set(entry.cover.clone());
    let first = entry.vid.clone();
    if first.is_empty() {
        s.buffering.set(false);
        s.error.set(t("player.episodeNoVid"));
        return;
    }
    log::info!(
        "[Player] 切剧《{}》series={} vid={}",
        entry.title,
        entry.series_id,
        first
    );
    prepare_and_play(s, &first, None);
    // 档案（分集 + 简介 + 互动回显）并行跟上；锚定正在播的 vid。
    load_archive(s, &entry.series_id, &first);
}

/// 换剧（供选集浮层的系列剧 tab）：以剧为单位起播并锁定连播。
pub(crate) fn switch_series(s: &Stage, series_id: &str, title: &str, cover: &str) {
    batch(|| {
        s.binge.set(true);
        s.episodes.set(Vec::new());
        s.current_index.set(0);
        reset_media(s);
        s.info.title.set(title.to_owned());
        s.info.heat_text.set(String::new());
        s.info.season_tag.set(String::new());
        s.info.badge.set(String::new());
        s.info.intro.set(String::new());
    });
    s.rail_ctx.title.set(title.to_owned());
    s.rail_ctx.series_id.set(series_id.to_owned());
    s.backdrop.set(cover.to_owned());
    log::info!("[Player] 连播切剧 series={series_id}");
    // 锚定空 = 档案到了从第一集开播。
    load_archive(s, series_id, "");
}

/// 沉浸流步进：末尾先续拉一页再切（不空转）。
fn step_series(s: &Stage, delta: i32) {
    let count = s.items.with(|list| list.len());
    if count == 0 {
        return;
    }
    let next = s.index.get() as i32 + delta;
    if next >= count as i32 {
        if s.has_more.with(|v| *v) {
            let tab = FeedNav::from_context().current().get();
            fetch_feed(s, &tab, true);
        }
        return;
    }
    if next < 0 {
        return;
    }
    play_series_at(s, next as usize);
}

/// 步进一集：-1 上一集 / +1 下一集；末集剧终换下一部剧。
pub(crate) fn step_episode(s: &Stage, delta: i32) {
    let list = s.episodes.with(|v| v.clone());
    if list.is_empty() {
        return;
    }
    let at = list
        .iter()
        .position(|e| e.vid_index == s.current_index.get())
        .map(|at| at as i32)
        .unwrap_or(-1);
    let next = at + delta;
    if next < 0 {
        return;
    }
    match list.get(next as usize) {
        Some(episode) => play_episode(s, episode),
        None => step_series(s, 1),
    }
}

/// 选集浮层点选：跳到那一集（选集只负责跳，不动连播锁）。
pub(crate) fn pick_episode(s: &Stage, vid_index: u32) {
    if let Some(episode) = s
        .episodes
        .with(|list| list.iter().find(|e| e.vid_index == vid_index).cloned())
    {
        play_episode(s, &episode);
    }
}

/// 起播一集：prepare → 等注册 → 换源；弹幕池同步换。
fn play_episode(s: &Stage, episode: &Episode) {
    batch(|| {
        reset_media(s);
        s.current_index.set(episode.vid_index);
    });
    let want = {
        let d = s.definition.get();
        (d != 0).then_some(d)
    };
    prepare_and_play(s, &episode.vid, want);
    let danmaku = s.danmaku;
    let vid = episode.vid.clone();
    let series_id = s.series_id.get();
    spawn(async move {
        danmaku::load_for(danmaku, &vid, &series_id, crate::state::state()).await;
    });
}

/// 清媒体态（换集/换剧共用）。
fn reset_media(s: &Stage) {
    batch(|| {
        s.buffering.set(true);
        s.preparing.set(true);
        s.error.set(String::new());
        s.time.set(0.0);
        s.duration.set(0.0);
        s.playing.set(true);
        s.source.set(String::new());
        s.definitions.set(Vec::new());
    });
    s.danmaku.clear_screen();
}

/// prepare 一集并换源（注册等待在 `feed::prepare_url` 里）。
fn prepare_and_play(s: &Stage, vid: &str, want: Option<u32>) {
    let stage = s.clone();
    let vid = vid.to_owned();
    spawn(async move {
        let state = crate::state::state();
        let vid_for_url = vid.clone();
        let result = background(async move { feed::prepare_url(&state, &vid, want).await }).await;
        match result {
            Ok(Ok((picked, definitions))) => {
                let url = crate::supply::stream_url(picked, &vid_for_url);
                batch(|| {
                    stage.buffering.set(false);
                    stage.preparing.set(false);
                    stage.definitions.set(definitions);
                    stage.definition.set(picked);
                    stage.source.set(url);
                });
                log::info!("[Player] 起播 vid={vid_for_url} 档位={picked}");
            }
            Ok(Err(e)) => {
                log::warn!("[Player] 准备失败 vid={vid_for_url}: {e}");
                batch(|| {
                    stage.buffering.set(false);
                    stage.preparing.set(false);
                    stage.error.set(e);
                });
            }
            Err(e) => {
                log::warn!("[Player] 后台任务失败: {e}");
                batch(|| {
                    stage.buffering.set(false);
                    stage.preparing.set(false);
                    stage.error.set(e.to_string());
                });
            }
        }
    });
}

/// 清晰度切换后重跑当前集。
pub(crate) fn replay_episode(s: &Stage) {
    let episode = s.episodes.with(|list| {
        list.iter()
            .find(|e| e.vid_index == s.current_index.get())
            .cloned()
    });
    if let Some(episode) = episode {
        play_episode(s, &episode);
    }
}

/// 进度条 seek（0..1 比例）。
pub(crate) fn seek_ratio(s: &Stage, ratio: f32) {
    let total = s.duration.get();
    if total > 0.0 {
        seek_to(s, ratio.clamp(0.0, 1.0) * total);
    }
}

/// seek 到指定秒。
pub(crate) fn seek_to(s: &Stage, seconds: f32) {
    let total = s.duration.get();
    let target = if total > 0.0 {
        seconds.min(total)
    } else {
        seconds.max(0.0)
    };
    s.time.set(target);
    s.player.seek(target as f64);
}

/// 播完一集：评论区开着 = 循环重播本集（官方同款）；否则自动连播
/// （设置可关）——本剧下一集（进入连播锁），末集换下一部剧。
pub(crate) fn on_end(s: &Stage) {
    if s.comments.open.get() {
        seek_to(s, 0.0);
        s.playing.set(true);
        return;
    }
    let auto_next = crate::state::state().settings().auto_next_episode;
    if auto_next {
        if !s.binge.get() {
            // 自动连播完成本集 = 进入选中剧连播态（入口之三）。
            s.binge.set(true);
        }
        step_episode(s, 1);
    }
}

/// 发弹幕（控制栏发送框）：未登录指路；成功乐观入池。
pub(crate) fn send_danmaku(s: &Stage) {
    if !s.logged_in.get() {
        toast(t("player.interact.loginRequired"), ToastDuration::Long);
        return;
    }
    let text = s.dm_draft.with(|v| v.trim().to_owned());
    let group = s.rail_ctx.bare_vid.get();
    let book = s.series_id.get();
    if text.is_empty() || group.is_empty() || book.is_empty() {
        return;
    }
    let offset_ms = (s.time.get() * 1000.0) as u64;
    s.dm_draft.set(String::new());
    let stage = s.clone();
    spawn(async move {
        let state = crate::state::state();
        let body = text.clone();
        let result = background(async move {
            hongguo_api::interact::send_danmaku(&group, &book, &body, offset_ms, &state.api_env())
                .await
        })
        .await;
        match result {
            Ok(Ok(_)) => {
                stage.danmaku.insert_local(text, offset_ms);
                toast(t("player.interact.danmakuSent"), ToastDuration::Short);
            }
            Ok(Err(e)) => log::warn!("[Player] 弹幕发送失败: {e}"),
            Err(e) => log::warn!("[Player] 弹幕发送任务失败: {e}"),
        }
    });
}

/// 档案并行装载：分集表 + 简介 + 互动回显（相关系列在选集浮层打开时按需拉）。
/// `anchor_vid` 是已在播的那一集——分集表到达后按它定位集号（左下角标、
/// 选集高亮、上下集边界都依赖它）。
fn load_archive(s: &Stage, series_id: &str, anchor_vid: &str) {
    let stage = s.clone();
    let sid = series_id.to_owned();
    let anchor = anchor_vid.to_owned();
    spawn(async move {
        let state = crate::state::state();
        let listed = background(async move { feed::fetch_series(&sid, &state).await }).await;
        match listed {
            Ok(Ok(list)) => {
                batch(|| {
                    stage.episodes.set(list.episodes.clone());
                    stage.info.title.set(list.title.clone());
                    stage.info.episode_count.set(list.episodes.len() as u32);
                    stage.rail.collect_count.set(list.followed_cnt);
                });
                // 正在播的这集对齐到分集表里的集号（左下角标、选集高亮、
                // 上下集边界都依赖它）；anchor 为空 = 换剧路径，档案到了
                // 从第一集开播（play_series_at 路径已在播，不动）。
                let start = list
                    .episodes
                    .iter()
                    .find(|e| e.vid == anchor)
                    .or_else(|| anchor.is_empty().then(|| list.episodes.first()).flatten());
                if let Some(episode) = start.cloned() {
                    stage.current_index.set(episode.vid_index);
                    if anchor.is_empty() && stage.source.get().is_empty() {
                        play_episode(&stage, &episode);
                    }
                }
                log::info!("[Player] 《{}》共 {} 集", list.title, list.episodes.len());
            }
            Ok(Err(e)) => log::warn!("[Player] 分集表拉取失败: {e}"),
            Err(e) => log::warn!("[Player] 分集表任务失败: {e}"),
        }
    });

    let stage = s.clone();
    let sid = series_id.to_owned();
    spawn(async move {
        let state = crate::state::state();
        let meta = background(async move { feed::fetch_meta(&sid, &state).await }).await;
        if let Ok(Ok(meta)) = meta {
            batch(|| {
                stage.info.intro.set(meta.intro);
                // 追更钮只在连载剧显示（官方同款判定源）。
                stage.rail.show_follow.set(meta.series_status == 0);
            });
        }
    });

    let stage = s.clone();
    spawn(async move {
        let state = crate::state::state();
        rail::restore_state(stage.rail, stage.rail_ctx, state).await;
    });
}

/// 选集浮层打开时拉相关系列（系列剧 tab）。
pub(crate) fn load_related(s: &Stage) {
    if !s.related.get().is_empty() {
        return;
    }
    let stage = s.clone();
    let sid = s.series_id.get();
    spawn(async move {
        let state = crate::state::state();
        let related = background(async move {
            hongguo_api::detail::fetch_related_series(&sid, &state.api_env())
                .await
                .map_err(|e| e.to_string())
        })
        .await;
        if let Ok(Ok(related)) = related {
            let mut works = related.works;
            // plan/v 不回正在播的这部：从档案合成一份补进头部（Tauri 同款）。
            let current = stage.series_id.get();
            if !works.iter().any(|w| w.series_id == current) {
                works.insert(
                    0,
                    RelatedItem {
                        series_id: current,
                        title: stage.info.title.get(),
                        cover: stage.backdrop.get(),
                        ..RelatedItem::default()
                    },
                );
            }
            works.sort_by_key(|w| season_no(&w.tag));
            stage.related.set(works);
        }
    });
}

/// 「第1季」→ 1（排序键；无季号的排最前）。
fn season_no(tag: &str) -> u32 {
    tag.trim_start_matches('第')
        .trim_end_matches('季')
        .parse()
        .unwrap_or(0)
}

/// 推荐流一页（`append` 只续拉，不动正在播的；首批到手起播第一部）。
pub(crate) fn fetch_feed(s: &Stage, tab: &str, append: bool) {
    if s.loading.get_untracked() {
        return;
    }
    s.loading.set(true);
    let offset = if append {
        s.next_offset.with(|v| *v)
    } else {
        0
    };
    let sid = if append {
        s.session.with(|v| v.clone())
    } else {
        String::new()
    };
    let stage = s.clone();
    let tab = tab.to_owned();
    spawn(async move {
        let state = crate::state::state();
        let tab_for_log = tab.clone();
        let result =
            background(async move { feed::fetch_feed(&tab, &sid, offset, &state).await }).await;
        match result {
            Ok(Ok(page)) => {
                let count = page.items.len();
                let boot = !append && count > 0;
                batch(|| {
                    stage.session.replace(page.session_id);
                    stage.next_offset.replace(page.next_offset);
                    stage.has_more.replace(page.has_more);
                    if append {
                        stage.items.update(|list| list.extend(page.items));
                    } else {
                        stage.items.set(page.items);
                    }
                    stage.loading.set(false);
                });
                log::info!("[Player] tab={tab_for_log} 推荐流 {count} 条（offset={offset}）");
                if boot {
                    play_series_at(&stage, 0);
                }
            }
            Ok(Err(e)) => {
                log::warn!("[Player] 推荐流失败: {e}");
                batch(|| {
                    stage.loading.set(false);
                    if !append && stage.items.get().is_empty() {
                        stage.error.set(format!("{}：{e}", t("common.loadFailed")));
                    }
                });
            }
            Err(e) => {
                log::warn!("[Player] 推荐流任务失败: {e}");
                batch(|| {
                    stage.loading.set(false);
                    if !append && stage.items.get().is_empty() {
                        stage.error.set(format!("{}：{e}", t("common.loadFailed")));
                    }
                });
            }
        }
    });
}
