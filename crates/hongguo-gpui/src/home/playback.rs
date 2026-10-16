//! 播放编排：起播链（档案 → 起点决策 → prepare → 引擎 open）、滚轮/连播
//! 切换、切清晰度、自动连播、进度落盘与流/档案预取。

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use hongguo_api::history::WatchHistoryItem;
use hongguo_common::AppResult;
use hongguo_playback::{OpenOptions, PlayerEvent, PlayerHandle};
use hongguo_service::app_state::AppStateInner;
use hongguo_service::play_service::online::prepare;
use hongguo_service::play_service::position;
use tokio::sync::mpsc;

use super::NowPlaying;
use super::feed::StreamItem;
use crate::bridge;

/// 播放进度落盘最小间隔（秒）（Tauri 侧同为 5 秒一写）。
const PROGRESS_SAVE_EVERY_SECS: u64 = 5;
/// 滚轮切剧去抖（毫秒）：滚轮一格连发多事件的吸收窗口。
const WHEEL_SWITCH_DEBOUNCE_MS: u64 = 400;

/// 一次起播请求的完整参数（结构体承载，[`Self::play_task`] 的参数面收敛）。
struct PlayRequest {
    item: StreamItem,
    /// 连播指定集号（None = 走起点决策）。
    forced_next_ep: Option<u32>,
    /// 用户显式选择的清晰度（None = 自动/沿用），传给 prepare。
    preferred_definition: Option<u32>,
    /// 起播倍速：引擎出生即按它走（见 `OpenOptions::rate`）。
    rate: f64,
    /// 切清晰度保进度的位置覆盖（None = 走起点决策）。
    start_override: Option<f64>,
}

/// 观看历史首报门槛（秒）：起播 ≥5 秒才记第一条（Tauri 同款，防噪点）。
const HISTORY_FIRST_REPORT_SECS: f64 = 5.0;

impl super::HomeView {
    /// 连播锁定内的切集（滚轮/↑↓，Tauri useBingeRelay 的 stepEpisode 同款）：
    /// 越过第 1 集原地不动；滚过末集 = 剧终，走剧终接力（下一季 → 信息流
    /// 下一条推荐）。
    pub(super) fn step_episode(&mut self, dir: i32, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        let next = now.vid_index as i64 + dir as i64;
        if next < 1 {
            return;
        }
        if next as usize > now.episode_count {
            // 滚过末集也算剧终（Tauri Ns()→hc() 同款）
            self.advance_after_series_end(cx);
            return;
        }
        if let Some(item) = self
            .display_items()
            .iter()
            .find(|i| i.series_id == now.series_id)
            .cloned()
        {
            self.play_target(item, Some(next as u32), None, cx);
        }
    }

    /// 手动重试（失败卡的「重试」按钮）：起播失败重走当前游标位的起播链
    ///（集号/起点重新走起点决策）；播放中失败（引擎自停、now 就是失败的
    /// 那部）则同集重播——play_target 的同剧早退只拦 forced_next_ep=None
    /// 的路径，带集号才能重进去。
    pub(super) fn retry_play(&mut self, cx: &mut Context<Self>) {
        let cursor_item = self.display_items().get(self.cursor()).cloned();
        if let (Some(now), Some(item)) = (self.now.clone(), cursor_item.as_ref())
            && now.series_id == item.series_id
        {
            self.play_target(item.clone(), Some(now.vid_index), None, cx);
            return;
        }
        if let Some(item) = cursor_item {
            self.play_target(item, None, None, cx);
        }
    }

    /// 离开首页的卸载清理（Tauri 路由切换 = 组件卸载的同语义）：强落进度、
    /// 释放引擎、收起浮层。HomeView 实体常驻带来「信息流/游标/封面缓存不
    /// 丢」，但 Tauri 卸载免费提供的停播面必须在这里补齐——不然解码、
    /// 声音、自动连播、5 秒落盘在切走后照常跑（切菜单还在播的根源）。
    /// 回流态不动：回首页由 [`Self::resume_if_idle`] 从落盘进度接上。
    pub(crate) fn release_for_leave(&mut self, cx: &mut Context<Self>) {
        if self.player.is_none() {
            return;
        }
        // 先落进度再放引擎：读数来自句柄，Drop 之后就是 0 了
        self.save_progress_now();
        // 陈旧事件与在途起播任务按纪元弃置；句柄 Drop = 任务 abort、音频
        // 设备释放、事件通道关闭（循环自灭）
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.player = None;
        self.frame = None;
        #[cfg(target_os = "windows")]
        self.clear_frame_image();
        self.switching = false;
        self.buffering = false;
        self.scrubbing = None;
        self.silent_until_input = false;
        self.controls_until = None;
        self.pending_title = None;
        self.close_overlays();
        log::info!("[Home] 离开首页：引擎已释放，进度已落盘");
        cx.notify();
    }

    /// 回到首页的续播（Tauri 重挂载「挂载默认开闸」同语义）：引擎不在
    /// （被 [`Self::release_for_leave`] 释放 / 冷启动还没起）就按当前游标
    /// 重走起点决策——`resolve_entry` 从落盘进度接上，同一部剧同一位置。
    /// 信息流还没就位时不动，首批落定的冷启动钩子（after_feed_loaded）
    /// 会接管。
    pub(crate) fn resume_if_idle(&mut self, cx: &mut Context<Self>) {
        // 回首页的过期换一批（Tauri FEED_STALE_MS 同款）：流数据比 5 分钟
        // 更旧就重开会话取新一批——在播剧由 pin_hold 钉在最前继续播（走
        // 同部剧重走起点决策的通道），滚下去才是新一批。没有在播目标
        // （冷启动首次）不换，与 Tauri 的 `playingId` 闸同位。
        if self.now.is_some() && self.feed_stale() {
            log::info!("[Home] 回首页：流已过期（>5 分钟），换一批（在播不动）");
            self.pin_hold = true;
            self.load_feed(self.active.clone(), true, cx);
            // 刷新会清列表并把游标归零：目标仍是「在播的那部」，把游标对回
            // 它在展示列表里的位置（已被换出则 pin 在最前，通常即 0；还在
            // 种子/新列表里则对到那一位）——Tauri 重挂载后的游标恢复同语义
            if let Some(sid) = self.now.as_ref().map(|n| n.series_id.clone())
                && let Some(pos) = self.display_items().iter().position(|i| i.series_id == sid)
            {
                self.set_cursor(pos);
            }
        }
        if self.engine_ready() || self.switching {
            return;
        }
        let Some(item) = self.display_items().get(self.cursor()).cloned() else {
            return;
        };
        // 控制条亮一拍（Tauri 重挂载 chrome 初始可见同款）
        self.controls_until = Some(Instant::now() + Self::CONTROLS_VISIBLE);
        self.play_target(item, None, None, cx);
    }

    /// 收起全部浮层/弹层（卸载语义：Tauri 重挂载后一切面板都是关着的）。
    fn close_overlays(&mut self) {
        self.picker_open = false;
        self.comments_panel_open = false;
        self.downloads_open = false;
        self.dm_panel_open = false;
        self.danmaku_emoji_open = false;
        self.emoji_picker_open = false;
        self.volume_popup_open = false;
        self.volume_hovered = false;
        self.volume_popup_hovered = false;
        self.rate_menu_open = false;
        self.definition_menu_open = false;
        self.controls_hovered = false;
        self.pointer_on_stage = false;
        self.reply_to = None;
    }

    /// 滚轮冷却裁决 + 记账：到点可切返回 true 并落章。只由滚轮路径调用    /// （键盘 ↑↓ 不吃这把锁）；换剧与连播切集共用——冷却原写在换剧分支
    /// 里，连播切集漏网，一次惯性滚动连跳好几集。
    pub(super) fn wheel_cooldown_ready(&mut self) -> bool {
        let now = Instant::now();
        if self.wheel_at.is_some_and(|t| {
            now.duration_since(t) < Duration::from_millis(WHEEL_SWITCH_DEBOUNCE_MS)
        }) {
            return false;
        }
        self.wheel_at = Some(now);
        true
    }

    /// 滚轮切剧。连播锁定时语义变为切集，不跟随换剧（冷却见
    /// [`Self::wheel_cooldown_ready`]，由舞台层滚轮入口先行裁决）。
    pub(super) fn step(&mut self, dir: i32, cx: &mut Context<Self>) {
        if self.in_binge() {
            // Tauri 同款：滚轮也是「用户在场」，切集时唤醒悬浮层
            self.wake_controls(cx);
            self.step_episode(dir, cx);
            return;
        }
        let len = self.display_items().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor();
        let next = (cur as i64 + dir as i64).clamp(0, len as i64 - 1) as usize;
        if next == cur {
            return;
        }
        self.set_cursor(next);
        // 快滑到尾部：预取下一页
        let feed = self.feeds.get(&self.active).expect("源已初始化");
        let should_load_more = feed.has_more && !feed.fetching_more && len - next <= 3;
        if should_load_more {
            self.load_more(self.active.clone(), cx);
        }
        if let Some(item) = self.display_items().get(next).cloned() {
            self.play_target(item, None, None, cx);
        }
        cx.notify();
    }

    /// 切到目标剧。旧的引擎继续播到新引擎就位（对齐 Tauri「store 目标
    /// 变了但画面还在上一部」的切换语义），就位即整体换装。
    /// `start_override` 给切清晰度保进度用（None = 走起点决策）。
    pub(super) fn play_target(
        &mut self,
        item: StreamItem,
        forced_next_ep: Option<u32>,
        start_override: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        // 「在播不动」闸：目标就是在播的那部才拦。引擎不在（被
        // `release_for_leave` 释放）时不算在播——回首页续播同部剧必须
        // 放行重走起点决策，否则从落盘进度接上的通道永远打不开。
        if self
            .now
            .as_ref()
            .is_some_and(|n| n.series_id == item.series_id)
            && forced_next_ep.is_none()
            && (self.engine_ready() || self.switching)
        {
            return;
        }
        // 起播目标即身份：出问题（画面错位/内容重复）时这行是唯一能对齐
        // 「屏幕上那部剧」与「引擎在放哪部」的证据。
        log::info!(
            "[Home] 起播目标 {}（{}）epoch={} 标题={}",
            item.series_id,
            match forced_next_ep {
                Some(ep) => format!("连播第 {ep} 集"),
                None => "起点决策".to_string(),
            },
            self.epoch.load(Ordering::Relaxed) + 1,
            if item.title.is_empty() {
                "<无标题>"
            } else {
                item.title.as_str()
            }
        );
        // 播放期系统活动断言（无条件持有，直到用户暂停）：无声/被遮挡/
        // 后台的进程会被 macOS 深度节流——换集起播的 50ms 轮询实测被拉长
        // 到 20s、GPUI 渲染循环整个停摆。断言必须挂在**节流打不到的入口**
        // （用户/连播事件的 UI 路径），不能挂在 render——被节流的恰恰是
        // render，翻沿永远走不到。暂停即释放（用户在场，恢复由
        // toggle_pause 重新持有）。
        crate::play_activity::set_active(true);
        self.switching = true;
        // 新一次起播尝试：清上一轮的失败态（自动重试的记账位只在引擎
        // 就位时复位——在这里也清会让「失败→重试→失败」死循环）
        self.play_error = None;
        // 批量取流预取（hgplayer 同款时机与窗口）：当前 + 信息流后续几部的
        // 推荐位 vid 一次批量；命中落 api 层暂存表，起播链先查它。批量
        // 空响应（当前服务端常态）时静默，起播照走单集端点。
        let batch_vids: Vec<String> = {
            let items = self.display_items();
            let cur = self.cursor();
            let mut vids: Vec<String> = Vec::new();
            if !item.vid.is_empty() {
                vids.push(item.vid.clone());
            }
            for it in items.iter().skip(cur + 1) {
                if vids.len() >= 5 {
                    break;
                }
                if !it.vid.is_empty() && !vids.contains(&it.vid) {
                    vids.push(it.vid.clone());
                }
            }
            vids
        };
        if batch_vids.len() > 1 {
            let state = Arc::clone(&self.state);
            bridge::spawn(async move {
                let env = state.api_env();
                let cap = hongguo_api::stream_pick::DecoderCapability::from_hevc(
                    hongguo_service::capability_service::hevc_playable(),
                );
                match hongguo_api::play_url::fetch_play_batch(&batch_vids, cap, &env).await {
                    Ok(n) if n > 0 => log::info!("[Home] 批量预取命中 {n} 条"),
                    Ok(_) => {}
                    Err(e) => log::warn!("[Home] 批量预取失败: {e}"),
                }
            });
        }
        // 旧的帧立即退场：切换期间画面上是新目标的封面，不是上一部的残帧
        self.frame = None;
        #[cfg(target_os = "windows")]
        self.clear_frame_image();
        self.pending_title = Some(if item.title.is_empty() {
            "正在准备…".to_string()
        } else {
            item.title.clone()
        });
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.load_cover_of(&item, cx);
        cx.notify();

        let state = Arc::clone(&self.state);
        let history = self.history.clone().unwrap_or_default();
        let (events, rx) = mpsc::channel::<PlayerEvent>(64);
        let epoch = self.epoch.load(Ordering::Relaxed);
        let epoch_shared = Arc::clone(&self.epoch);
        // 事件循环先于引擎起：Channel 关闭（引擎丢弃 → 发送端清零）即自然退出
        #[cfg(target_os = "windows")]
        let video_target = Arc::clone(&self.video_target);
        #[cfg(target_os = "windows")]
        self.spawn_event_loop(epoch, epoch_shared, rx, video_target, cx);
        #[cfg(not(target_os = "windows"))]
        self.spawn_event_loop(epoch, epoch_shared, rx, cx);
        let done = bridge::spawn(Self::play_task(
            state,
            history,
            PlayRequest {
                item,
                forced_next_ep,
                preferred_definition: self.preferred_definition,
                rate: self.rate,
                start_override,
            },
            events,
        ));
        cx.spawn(async move |this, cx| {
            let result = done.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                // 陈旧任务（期间用户又切了剧）：不接管，引擎随 result 丢弃自停
                if view.epoch.load(Ordering::Relaxed) != epoch {
                    return;
                }
                match result {
                    Ok((handle, now)) => {
                        // 新引擎就位（帧随后由它的 Frame 事件送到，这里不清帧：
                        // 起了才清的写法会把先到的首帧抹掉，封面从此收不掉）。
                        // 切换途中旧帧已清，到这里的画面本来就是封面占位。
                        // 偏好重放：换集重建引擎（对齐 Tauri `<video>` 卸载重挂）后
                        // 把音量/静音按存值发给新引擎。倍速已随 OpenOptions 出生
                        // 即位（见 play_task），只在分歧时补发——同值补发会平白
                        // 走一次分级换轴记账。
                        if (handle.rate() - view.rate).abs() > 1e-9 {
                            handle.set_rate(view.rate);
                        }
                        handle.set_volume(view.volume);
                        handle.set_muted(view.muted);
                        // 换引擎前的 force 落盘 + 观看历史（Tauri 切集
                        // persist(force) 同款）：旧引擎与旧 now 还在手里，
                        // 读数有效；首次起播 player 为空自然跳过
                        view.save_progress_now();
                        view.player = Some(handle);
                        // 连播锁定跟着目标走（Tauri setTarget 同款）：切到
                        // 别的剧即解除，同剧连播保持
                        if view.binge.as_deref() != Some(now.series_id.as_str()) {
                            view.binge = None;
                        }
                        view.load_intro(now.series_id.clone(), cx);
                        // 剧终接力的下一季数据面随起播预取（Tauri 起播时
                        // 拉好相关列表同款；剧终时通常已就绪）
                        view.load_related_seasons(now.series_id.clone(), cx);
                        // 互动数据：集级公开计数（档案）+ mget 最近互动
                        view.load_episode_counts(&now, cx);
                        view.load_interact_state(cx);
                        // 弹幕按集重拉（社交态不缓存，每次进集都拉）。必须在
                        // now 就位之后、且 series_id 显式传参：以前先于 now 赋值
                        // 调用、内部又依赖 self.now——首播时 now 还是 None 直接
                        // 返回（第一部剧永远没弹幕），换剧时错拿上一部剧的
                        // series_id 当 book_id（服务端参数错配，热剧也拉空）
                        view.load_danmaku(now.vid.clone(), now.series_id.clone(), cx);
                        view.now = Some(now);
                        view.switching = false;
                        view.buffering = false;
                        view.play_retried = false;
                        log::info!("[Home] 引擎就位（epoch={epoch}），switching 解除");
                    }
                    Err(e) => {
                        log::warn!("[Home] 起播失败: {e}");
                        view.switching = false;
                        view.buffering = false;
                        // 对齐 Tauri recovery-policy 的 retry 档：自动重试一次
                        //（换目标/换集失败同样算一次预算），再失败才落错误态
                        if !view.play_retried {
                            view.play_retried = true;
                            log::info!("[Home] 起播失败，自动重试一次");
                            if let Some(item) = view.display_items().get(view.cursor()).cloned() {
                                view.play_target(item, None, None, cx);
                                cx.notify();
                                return;
                            }
                        }
                        // 失败必须可见：此前只写 pending_title，而那行小徽标
                        // 随 switching 消失——用户看到的就是「点了没反应」
                        view.play_error = Some(format!("起播失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 起播链（tokio 线程）：解析档案 → 起点决策 → prepare → 引擎 open。
    /// 与 Tauri 同一段服务面；起点 >3 秒续播，本地档案优先、云端历史兜底。
    async fn play_task(
        state: Arc<AppStateInner>,
        history: Vec<WatchHistoryItem>,
        req: PlayRequest,
        events: mpsc::Sender<PlayerEvent>,
    ) -> AppResult<(std::sync::Arc<PlayerHandle>, NowPlaying)> {
        let PlayRequest {
            item,
            forced_next_ep,
            preferred_definition,
            rate,
            start_override,
        } = req;
        let env = state.api_env();

        let series = match state.store.series_by_id(&item.series_id)? {
            Some(s) if !s.episodes.is_empty() => s,
            _ => {
                let s = hongguo_service::series_service::resolver::resolve_series(
                    &item.series_id,
                    &env,
                )
                .await?;
                state.store.upsert_series(&s)?;
                s
            }
        };
        let episode_count = series.episodes.len();
        // 切档保进度：覆盖值优先于起点决策（覆盖必然伴随显式集号——
        // 切清晰度 = 同集同位重播）
        let (vid_index, start_secs) = match (start_override, forced_next_ep) {
            (Some(pos), forced) => (forced.unwrap_or(1), pos),
            (None, Some(idx)) => (idx, 0.0),
            (None, None) => hongguo_service::play_service::resume::resolve_entry(
                &state.store,
                &item.series_id,
                &history,
            )?,
        };
        // 找集号：连播是「上一集接着往下」——目标集不存在时必须报错，绝不能
        // 回落到第 1 集（那是「又从头播一遍」的来源，用户看到的正是重复）。
        // 只有起点决策（forced_next_ep 为空）才允许回落到第 1 集。
        let ep = match series.episodes.iter().find(|e| e.vid_index == vid_index) {
            Some(ep) => ep,
            None if forced_next_ep.is_some() => {
                return Err(hongguo_common::AppError::Media(format!(
                    "第 {vid_index} 集不在分集表里（表里 {} 集），不回落重播",
                    series.episodes.len()
                )));
            }
            None => series
                .episodes
                .iter()
                .find(|e| e.vid_index == 1)
                .ok_or_else(|| hongguo_common::AppError::Media("分集表里没有可播的集".into()))?,
        };
        if ep.vid.trim().is_empty() {
            return Err(hongguo_common::AppError::Media("该集没有 vid".into()));
        }
        let progress_key = format!("{}:{}", item.series_id, ep.vid_index);
        // 实际要播的 vid：比对「同一集被播了两遍」与「恰好落在第一集」的
        // 唯一凭证（重复内容投诉时先看这行，再看起播目标那行）
        log::info!(
            "[Home] 起播 {} 第 {} 集（vid={}）起点 {:.1}s",
            item.series_id,
            ep.vid_index,
            ep.vid,
            start_secs
        );
        // 本地成品优先（Tauri `play_service::local::resolve_play` 同款）：这一集
        // 已经下好就直接读本地文件——离线可播、秒开，且没有档位概念（本地只有
        // 一版，清晰度菜单该是灰的）。记录里路径存在但文件没了（陈旧记录）时
        // 回落在在线流；在线分流（prepare）只在真的没有本地成品时才走。
        let local_path = state
            .queue()
            .completed_path(&item.series_id, ep.vid_index)
            .filter(|p| !p.trim().is_empty() && std::path::Path::new(p).is_file());
        let (source, definitions, definition) = match local_path {
            Some(path) => {
                log::info!("[Home] 本地成品命中，走本地播放：{path}");
                (
                    hongguo_playback::PlaySource::Local { path: path.into() },
                    Vec::new(),
                    0u32,
                )
            }
            None => {
                let prepared =
                    prepare(&state, &ep.vid, &progress_key, preferred_definition).await?;
                (
                    hongguo_playback::PlaySource::Online {
                        vid: ep.vid.clone(),
                        definition: prepared.definition,
                    },
                    prepared.definitions,
                    prepared.definition,
                )
            }
        };
        let handle = hongguo_playback::open(OpenOptions {
            source,
            start_secs,
            rate,
            events,
        })
        .await?;
        Ok((
            handle,
            NowPlaying {
                series_id: item.series_id,
                title: if item.title.is_empty() {
                    series.title.clone()
                } else {
                    item.title.clone()
                },
                vid_index: ep.vid_index,
                episode_count,
                heat_text: item.heat_text,
                season_tag: item.season_tag,
                badge: item.badge,
                definitions,
                definition,
                vid: ep.vid.clone(),
                cover: item.cover.clone(),
            },
        ))
    }

    /// 拉当前剧的简介、全剧收藏数与连载状态（video_detail 的 series_intro /
    /// followed_cnt / series_status，Tauri useSeriesDetailMeta 同源）：装饰性
    /// 信息，失败静默降级、允许重试；每剧只拉一次（会话内缓存，Tauri
    /// staleTime 10 分钟同语义）。followed_cnt 是互动栏「收藏」键的计数源；
    /// series_status 决定「追更」键显隐。
    fn load_intro(&mut self, series_id: String, cx: &mut Context<Self>) {
        // 切剧即重置上一部残值（计数/连载态/追更态），缓存命中就直接回填
        self.follow_count = 0;
        self.series_status = None;
        self.follow_is_on = None;
        if let Some((followed_cnt, status)) = self.series_meta_cache.get(&series_id) {
            self.follow_count = *followed_cnt;
            self.series_status = Some(*status);
        }
        self.load_follow_state(series_id.clone(), cx);
        if self.intros.contains_key(&series_id) || !self.intros_pending.insert(series_id.clone()) {
            return;
        }
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let sid = series_id.clone();
            hongguo_api::detail::fetch_series_meta(&sid, &env)
                .await
                .map(|meta| {
                    (
                        sid.clone(),
                        meta.intro,
                        meta.followed_cnt,
                        meta.series_status,
                    )
                })
                .map_err(|e| (sid, e))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| match result {
                Ok((sid, intro, followed_cnt, series_status)) => {
                    view.intros_pending.remove(&sid);
                    if !intro.is_empty() {
                        view.intros.insert(sid.clone(), intro);
                    }
                    view.series_meta_cache
                        .insert(sid.clone(), (followed_cnt, series_status));
                    // 快速切剧时响应会迟到：只在仍是当前剧时落 UI
                    if view.now.as_ref().is_some_and(|n| n.series_id == sid) {
                        view.follow_count = followed_cnt;
                        view.series_status = Some(series_status);
                    }
                }
                Err((sid, e)) => {
                    // 失败不算数：允许下次起播重试（rank.rs 同款记一条 warn）
                    view.intros_pending.remove(&sid);
                    log::warn!("[Home] {sid} 简介拉取失败，下次起播重试：{e}");
                }
            });
        })
        .detach();
    }

    /// 当前剧追更态（跟随列表首页本地判成员，service 同一段）：登录才查；
    /// 切剧时已重置为 None，回填前核对剧没被切走。
    fn load_follow_state(&mut self, series_id: String, cx: &mut Context<Self>) {
        if self.state.settings().account.is_none() {
            return;
        }
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            hongguo_service::follow_service::is_following(&state, &sid).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if view.now.as_ref().is_some_and(|n| n.series_id == series_id)
                    && let Ok(on) = result
                {
                    view.follow_is_on = Some(on);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 切清晰度（Tauri onDefinitionChange 同款）：同集同位重播，缓存按
    /// (vid, 档位) 分条目——切回去不用重新下载，也不动正在播的那一档。
    /// `None` = 回到自动（沿用上轮解析到的档位）。
    pub(super) fn switch_definition(&mut self, value: Option<u32>, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        // 与当前实际档位同值就不折腾（自动档位下点解析出的那一档也算同值）
        if value.is_none() && self.preferred_definition.is_none() {
            return;
        }
        if value == Some(now.definition) && self.preferred_definition == value {
            return;
        }
        let position = self
            .player
            .as_ref()
            .map(|p| p.position_secs())
            .unwrap_or(0.0);
        self.preferred_definition = value;
        self.wake_controls(cx);
        if let Some(item) = self
            .display_items()
            .iter()
            .find(|i| i.series_id == now.series_id)
            .cloned()
        {
            self.play_target(item, Some(now.vid_index), Some(position), cx);
        }
        cx.notify();
    }

    /// 选集浮层里点某一集：跳集，**不关浮层**（hgplayer 同款连选连跳）；
    /// 信息流里主动选集 = 锁定本剧连播（Tauri onPickEpisode 同款）。
    pub(super) fn pick_episode(&mut self, vid_index: u32, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        if vid_index != now.vid_index
            && let Some(item) = self
                .display_items()
                .iter()
                .find(|i| i.series_id == now.series_id)
                .cloned()
        {
            self.binge = Some(now.series_id.clone());
            self.play_target(item, Some(vid_index), None, cx);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 详情页播放入口（DetailEvent::Play → 壳 → 这里）：明确要追这部，
    /// 锁定连播；起播集与起点由详情页的继续播放决策给定。标题/封面从
    /// 档案抄（阻塞池读，UI 线程不碰库）。
    pub(crate) fn play_from_detail(
        &mut self,
        series_id: String,
        vid_index: u32,
        start_secs: f64,
        cx: &mut Context<Self>,
    ) {
        // 起点复核（Tauri 首页「成为当前剧」effect 同款）：列表页调用方只
        // 拿得到本地档案，云端观看历史 >3 秒时用云端位置兜底——传上来的
        // (1, 0.0) 正是「本地没有可续播记录」的形态，显式续播/换集
        // （idx>1 或带位置）不受影响。
        let (vid_index, start_secs) = if vid_index == 1 && start_secs == 0.0 {
            match hongguo_service::play_service::resume::resolve_entry(
                &self.state.store,
                &series_id,
                self.history.as_deref().unwrap_or(&[]),
            ) {
                Ok((idx, pos)) if idx > 1 || pos > 0.0 => (idx, pos),
                _ => (vid_index, start_secs),
            }
        } else {
            (vid_index, start_secs)
        };
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let done = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                state
                    .store
                    .series_by_id(&sid)
                    .ok()
                    .flatten()
                    .map(|s| (s.title, s.cover))
            })
            .await
            .expect("阻塞池 join")
        });
        cx.spawn(async move |this, cx| {
            let info = done.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                let (title, cover) = info.unwrap_or_default();
                let item = StreamItem {
                    series_id: series_id.clone(),
                    title,
                    cover,
                    horiz_cover: String::new(),
                    heat_text: String::new(),
                    season_tag: String::new(),
                    badge: String::new(),
                    vid: String::new(),
                };
                view.binge = Some(series_id.clone());
                view.play_target(
                    item,
                    Some(vid_index),
                    (start_secs > 0.0).then_some(start_secs),
                    cx,
                );
            });
        })
        .detach();
    }

    /// 看完自动删（设置页「看完自动删除」）：删掉刚看完那集的本地文件，
    /// 删到了才提示。文件删除 + 任务记录摘除都在 service 的 cleanup 里，
    /// 与 Tauri `deleteEpisode` 同源；同步文件 IO 走阻塞池，不堵 UI 线程。
    fn auto_delete_finished_episode(&mut self, cx: &mut Context<Self>) {
        if !self.state.settings().auto_delete_after_play {
            return;
        }
        let Some(now) = &self.now else {
            return;
        };
        let (series_id, vid_index) = (now.series_id.clone(), now.vid_index);
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            tokio::task::spawn_blocking(move || {
                hongguo_service::storage_service::cleanup::delete_episode(
                    &state, &series_id, vid_index,
                )
            })
            .await
            .unwrap_or(Ok(false))
        });
        cx.spawn(async move |this, cx| {
            let deleted = rx.await.unwrap_or(Ok(false)).unwrap_or(false);
            if !deleted {
                return;
            }
            let _ = this.update(cx, |view, cx| {
                view.show_toast(format!("第{vid_index}集已看完，已删除本地文件"), cx);
            });
        })
        .detach();
    }

    /// 连播：下一集；末集走剧终接力（下一季 → 信息流下一条推荐）。
    pub(super) fn advance_after_end(&mut self, cx: &mut Context<Self>) {
        // 看完自动删（Tauri handleEnded 的 autoDelete 同序：先删刚看完这集的
        // 本地文件，再决定连播——下载面板开着也照删，删除不依赖连播）。
        // 没下载过的集本来就没有文件，`delete_episode` 返回 false，不提示。
        self.auto_delete_finished_episode(cx);
        // 下载面板开着不自动连播（Tauri `autoNext && !downloading` 同款：
        // 面板挂着用户刚选完的集，切集/切剧会把勾选连同面板一起吃掉）——
        // 停在末帧等用户。
        if self.downloads_open {
            return;
        }
        self.frame = None;
        #[cfg(target_os = "windows")]
        self.clear_frame_image();
        // 防重入：Ended 只在时钟走完才发，但引擎收尾、通道重连都可能让同一集
        // 的 Ended 再来一次——那时 vid_index 还停在旧值，会把刚起播的下一集
        // 又推进一格（连着跳集，看着就是「内容来回变」）。切换途中直接吞掉。
        if self.switching {
            return;
        }
        // 自动连播静默起步（Tauri quietStart 同款）：新集首帧或用户真实输入
        // 之前，悬浮层一律不亮（首帧/on_player_event、wake_on_input 解除）。
        // 8s 硬上限（Tauri QUIET_WINDOW_MS 同款）：起播失败/首帧迟迟不来时，
        // 静默不能把悬浮层永久困死
        self.silent_until_input = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(8)).await;
            let _ = this.update(cx, |view, cx| {
                if view.silent_until_input {
                    view.silent_until_input = false;
                    cx.notify();
                }
            });
        })
        .detach();
        if let Some(now) = self.now.clone()
            && (now.vid_index as usize) < now.episode_count
        {
            let next_idx = now.vid_index + 1;
            if let Some(item) = self
                .display_items()
                .iter()
                .find(|i| i.series_id == now.series_id)
                .cloned()
            {
                // 自动连播完成本集 = 进入选中剧连播态（Tauri player-page
                // autoNext 分支 setBinge 同款）：滚轮/↑↓ 从这里起切集
                self.binge = Some(now.series_id.clone());
                self.play_target(item, Some(next_idx), None, cx);
                return;
            }
        }
        self.advance_after_series_end(cx);
    }

    /// 剧终接力（Tauri useBingeRelay.advanceAfterSeriesEnd 同款）：优先接
    /// **下一季**第 1 集（相关作品按季号挑「大于当前季的最小已上线季」），
    /// 没有下一季则跟随宿主（信息流）刷下一条推荐。本壳宿主恒为信息流，
    /// Tauri 无宿主签名下的「猜你喜欢」分支不适用。
    fn advance_after_series_end(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            self.step(1, cx);
            return;
        };
        // 下载面板开着不接力（同连播闸）——退回提示，静默不动像坏了
        if self.downloads_open {
            self.show_toast(format!("已经是最后一集（第{}集）了", now.episode_count), cx);
            return;
        }
        // 下一季数据面：起播时已预取（plan/v 相关作品；未到/拉失败 = 空表）
        let works = self
            .related_seasons
            .get(&now.series_id)
            .cloned()
            .unwrap_or_default();
        if let Some(next) =
            crate::home::season::pick_next_season(&works, &now.series_id, &now.title)
        {
            // hgplayer 同款：toast「即将播放下一季」+ 直接开播第 1 集，
            // 连播锁定延续到新季
            self.show_toast(format!("即将播放下一季：{}", next.title), cx);
            self.binge = Some(next.series_id.clone());
            self.play_target(
                StreamItem {
                    series_id: next.series_id.clone(),
                    title: next.title.clone(),
                    cover: next.cover.clone(),
                    ..Default::default()
                },
                Some(1),
                None,
                cx,
            );
            return;
        }
        // 没有下一季：跟随信息流刷下一条推荐（已到底时宿主原地驻留）
        self.step(1, cx);
    }

    /// 首帧出画：预取下一部剧续播集的流（每部一次；Tauri 首帧发令同款）。
    pub(super) fn prefetch_next_stream(&mut self, cx: &mut Context<Self>) {
        let items = self.display_items();
        let cursor = self.cursor();
        let Some(next) = items.get(cursor + 1).cloned() else {
            return;
        };
        if !self.stream_prefetched.insert(next.series_id.clone()) {
            return;
        }
        let state = Arc::clone(&self.state);
        let history = self.history.clone().unwrap_or_default();
        // 整条取流链跑在 tokio 桥里（reqwest/定时器要 reactor）；GPUI 侧只接结果
        let done = bridge::spawn(Self::prefetch_stream_task(
            state,
            next.series_id.clone(),
            history,
        ));
        cx.spawn(async move |this, cx| {
            if let Err(e) = done.await.expect("tokio 桥不会丢弃发送端") {
                // 失败不算数：下次再成为「下一部」时允许重试
                let _ = this.update(cx, |view, _| {
                    view.stream_prefetched.remove(&next.series_id);
                });
                log::info!("[Home] 预取下一部流失败（忽略）: {e}");
            }
        })
        .detach();
    }

    async fn prefetch_stream_task(
        state: Arc<AppStateInner>,
        series_id: String,
        history: Vec<WatchHistoryItem>,
    ) -> AppResult<()> {
        // 铺几集跟连播预取同一个设置档（0=全关，连换剧预取也不做）
        let count = state.settings().prefetch_episodes;
        if count == 0 {
            return Ok(());
        }
        let env = state.api_env();
        let series = match state.store.series_by_id(&series_id)? {
            Some(s) if !s.episodes.is_empty() => s,
            _ => {
                let s = hongguo_service::series_service::resolver::resolve_series(&series_id, &env)
                    .await?;
                state.store.upsert_series(&s)?;
                s
            }
        };
        let (vid_index, _) = hongguo_service::play_service::resume::resolve_entry(
            &state.store,
            &series_id,
            &history,
        )?;
        // 入口集不在分集表（下架/重排）回落第 1 集
        let entry = if series.episodes.iter().any(|e| e.vid_index == vid_index) {
            vid_index
        } else {
            1
        };
        // 与 Tauri 壳 play_prefetch 同一口径：入口集起共 N 集，逐集排队
        // schedule（幂等/去重/礼让在 schedule 内部）
        for offset in 0..count {
            hongguo_service::play_service::online::schedule_next_episode_prefetch(
                Arc::clone(&state),
                &series_id,
                entry + offset,
            );
        }
        Ok(())
    }

    /// 相邻剧分集档案预取（换剧时 resolve 秒回）。
    pub(super) fn prefetch_neighbor_archives(&mut self) {
        let items = self.display_items();
        let cursor = self.cursor();
        for idx in [cursor + 1, cursor.saturating_sub(1)] {
            let Some(item) = items.get(idx) else {
                continue;
            };
            let sid = item.series_id.clone();
            if sid.is_empty() || !self.archives_prefetched.insert(sid.clone()) {
                continue;
            }
            let state = Arc::clone(&self.state);
            bridge::spawn(async move {
                if state.store.series_by_id(&sid).ok().flatten().is_none() {
                    let env = state.api_env();
                    if let Ok(s) =
                        hongguo_service::series_service::resolver::resolve_series(&sid, &env).await
                    {
                        let _ = state.store.upsert_series(&s);
                    }
                }
            });
        }
    }

    /// 5 秒节流落盘（播放档案是真值：续播、详情页「继续看」都吃它）。
    pub(super) fn save_progress_maybe(&mut self) {
        if self.last_save.elapsed() < Duration::from_secs(PROGRESS_SAVE_EVERY_SECS) {
            return;
        }
        self.last_save = Instant::now();
        self.save_progress_inner(false);
    }

    /// 立即落一次进度（不带节流）：关小屏等「要走了」的时刻强制落，
    /// 别让最后一段播放丢在节流窗里（Tauri onPause 强落同款）；同时强制
    /// 记一条观看历史（切集/暂停/离开的 force 路径，Tauri 同款）。
    pub(super) fn save_progress_now(&mut self) {
        self.save_progress_inner(true);
    }

    fn save_progress_inner(&mut self, force_history: bool) {
        let Some((handle, now)) = self.player.as_ref().zip(self.now.clone()) else {
            return;
        };
        let (pos, dur) = (handle.position_secs(), handle.duration_secs());
        let store = self.state.store.clone();
        bridge::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                position::save_store(&store, &now.series_id, now.vid_index, pos, dur)
            })
            .await;
        });
        self.record_watch_history(force_history);
    }

    /// 观看历史记录（issue #10）：本地必记 + 登录态顺路上报云端（走
    /// `history_service::record`，与 Tauri record_watch_history 命令同一段
    /// service）。节奏与 Tauri `use-playback-progress` 的实际行为一致：
    /// **起播 ≥5 秒首报一次**，之后由 force 事件（暂停/切集/离开）补记；
    /// <5 秒的掠过一律不记（防「快滑一路全是 1 秒噪点行」）。
    fn record_watch_history(&mut self, force: bool) {
        let Some((handle, now)) = self.player.as_ref().zip(self.now.clone()) else {
            return;
        };
        let (pos, dur) = (handle.position_secs(), handle.duration_secs());
        if pos < HISTORY_FIRST_REPORT_SECS || now.vid.trim().is_empty() {
            return;
        }
        let key = format!("{}:{}", now.series_id, now.vid_index);
        if self.history_report_key != key {
            self.history_report_key = key;
            self.history_reported = 0;
        }
        if !force && self.history_reported > 0 {
            return;
        }
        self.history_reported += 1;
        let item = hongguo_common::model::LocalHistoryItem {
            series_id: now.series_id.clone(),
            title: now.title.clone(),
            cover: now.cover.clone(),
            vid_index: now.vid_index,
            vid: now.vid.clone(),
            position_ms: (pos * 1000.0).round() as i64,
            duration_ms: (dur * 1000.0).round() as i64,
            episode_cnt: now.episode_count as u32,
            updated_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        };
        let state = Arc::clone(&self.state);
        bridge::spawn(async move {
            if let Err(e) = hongguo_service::history_service::record(&state, &item).await {
                log::warn!("[Home] 观看历史记录失败（忽略）: {e}");
            }
        });
        log::debug!(
            "[Home] 观看历史记录 #{}（{}:{}, {pos:.1}s）",
            self.history_reported,
            now.series_id,
            now.vid_index
        );
    }
}
