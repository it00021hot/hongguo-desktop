//! 切剧/切集编排：滚轮与连播切换、起播目标、切片与选集、已看完跳过，
//! 以及离开首页的释放与回首页续播。

use super::*;

use super::open::{PlayRequest, SystemSpec};
use hongguo_common::AppError;

/// 官方批量预取窗口：当前 + 后续共 6 条（model/preload 与
/// detail/preload 一次各带 6 个）。
const OFFICIAL_PRELOAD_WINDOW: usize = 6;

impl super::HomeView {
    /// 连播锁定内的切集（滚轮/↑↓）：越过第 1 集原地不动；滚过末集 = 剧终，
    /// 走剧终接力（下一季 → 信息流下一条推荐）。
    pub(crate) fn step_episode(&mut self, dir: i32, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        let next = now.vid_index as i64 + dir as i64;
        if next < 1 {
            return;
        }
        if next as usize > now.episode_count {
            self.ended_generation = self.play_generation;
            log::info!(
                "[Home][尾进] 片尾前进 gen={} ep {}→剧终/下一部 source=step",
                self.play_generation,
                now.vid_index
            );
            self.advance_after_series_end(cx);
            return;
        }
        if let Some(item) = self
            .display_items()
            .iter()
            .find(|i| i.series_id == now.series_id)
            .cloned()
        {
            log::info!(
                "[Home][尾进] step gen={} ep {}→{next}",
                self.play_generation,
                now.vid_index
            );
            self.play_target(item, Some(next as u32), None, cx);
        }
    }

    /// 滚轮切剧。连播锁定时语义变为切集，不跟随换剧（冷却见
    /// [`Self::wheel_cooldown_ready`]，由舞台层滚轮入口先行裁决）。
    pub(crate) fn step(&mut self, dir: i32, cx: &mut Context<Self>) {
        if self.in_binge() {
            // 滚轮也是「用户在场」：切集时唤醒悬浮层。
            self.wake_controls(cx);
            self.step_episode(dir, cx);
            return;
        }
        let items = self.display_items();
        let len = items.len();
        if len == 0 {
            return;
        }
        let cur = self.cursor();
        let mut next = (cur as i64 + dir as i64).clamp(0, len as i64 - 1) as usize;
        if next == cur {
            return;
        }
        // 已看完的剧不播、不预热：沿方向跳到下一部未完成的；整段都看完
        // 就停在边界，不创建播放器（加载条也随之关闭）。
        let mut hops = 0usize;
        while self.series_finished(&items[next].series_id) {
            hops += 1;
            if hops > len {
                self.set_cursor(next);
                cx.notify();
                return;
            }
            let candidate = next as i64 + dir as i64;
            if candidate < 0 || candidate >= len as i64 {
                self.set_cursor(next);
                cx.notify();
                return;
            }
            next = candidate as usize;
        }
        self.set_cursor(next);
        // 快滑到尾部：预取下一页
        let feed = self.feeds.get(&self.active).expect("源已初始化");
        let should_load_more = feed.has_more && !feed.fetching_more && len - next <= 3;
        if should_load_more {
            self.load_more(self.active.clone(), cx);
        }
        if let Some(item) = items.get(next).cloned() {
            self.play_target(item, None, None, cx);
        }
        cx.notify();
    }

    /// 滚轮冷却裁决 + 记账：到点可切返回 true 并落章。只由滚轮路径调用
    /// （键盘 ↑↓ 不吃这把锁）；换剧与连播切集共用。
    pub(in crate::home) fn wheel_cooldown_ready(&mut self) -> bool {
        let now = Instant::now();
        if self.wheel_at.is_some_and(|t| {
            now.duration_since(t) < Duration::from_millis(WHEEL_SWITCH_DEBOUNCE_MS)
        }) {
            return false;
        }
        self.wheel_at = Some(now);
        true
    }

    /// 收起全部浮层/弹层（卸载语义）。
    fn close_overlays(&mut self) {
        self.picker_open = false;
        self.comments_panel_open = false;
        self.downloads_open = false;
        self.dm_panel_open = false;
        self.danmaku_emoji_open = false;
        self.danmaku_compact_open = false;
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

    /// 切到目标剧：旧的引擎继续播到新引擎就位，就位即整体换装。
    /// `start_override` 给切清晰度保进度用（None = 走起点决策）。
    pub(in crate::home) fn play_target(
        &mut self,
        item: StreamItem,
        forced_next_ep: Option<u32>,
        start_override: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        // 「在播不动」闸：目标就是在播的那部才拦；引擎不在（被
        // release_for_leave 释放）不算在播——回首页续播同部剧必须放行。
        if self
            .now
            .as_ref()
            .is_some_and(|n| n.series_id == item.series_id)
            && forced_next_ep.is_none()
            && (self.engine_ready() || self.switching)
        {
            return;
        }
        // 信息流/历史点到已看完的剧：不创建播放器、不 prepare/prefetch，
        // 直接跳下一部未看完的。
        if forced_next_ep.is_none()
            && start_override.is_none()
            && self.series_finished(&item.series_id)
        {
            log::info!("[Home] 已看完《{}》→ 播放路径跳过", item.series_id);
            self.skip_finished_series(&item.series_id, cx);
            return;
        }
        // 每次起播/接力开新代次：片尾幂等以代次为准（AVQueuePlayer 接力
        // 不换播放器，旧 Ended/旧尾部读数必须按代次隔离）。
        self.play_generation = self.play_generation.wrapping_add(1);
        // 系统后端连播下一集：队列里已预加载就直接接力，不重建播放器
        //（重建要走取流表+asset+loader，慢一个量级）。
        if start_override.is_none()
            && self.preferred_definition.is_none_or(|definition| {
                self.now
                    .as_ref()
                    .is_some_and(|now| now.definition == definition)
            })
            && forced_next_ep == self.now.as_ref().map(|now| now.vid_index + 1)
            && self
                .now
                .as_ref()
                .is_some_and(|now| now.series_id == item.series_id)
            && let Some(next_index) = forced_next_ep
        {
            // 下一集的 vid 取自起播时缓存的分集表（主线程不碰库）：这里要的
            // 正是当初预加载进队列的那一条，缓存里的它比再读一次库更准。
            let next_vid = self.now.as_ref().and_then(|now| {
                now.episode_vids
                    .iter()
                    .find(|(index, _)| *index == next_index)
                    .map(|(_, vid)| vid.clone())
            });
            if let (Some(next_vid), Some(player)) = (next_vid, self.player.as_ref())
                && !next_vid.is_empty()
                && player.advance_preloaded(&next_vid)
            {
                log::info!(
                    "[Home][尾进] 接力 gen={} ep {}→{next_index}",
                    self.play_generation,
                    self.now.as_ref().map(|now| now.vid_index).unwrap_or(0)
                );
                // 缓存的「当前集」要跟着正在播的走：接力不走 prepare，
                // set_current 内会按预算逐出旧集（否则长会话无界堆流缓存）。
                hongguo_stream::cache().set_current(&next_vid);
                if let Some(now) = self.now.as_ref() {
                    let progress_key = format!("{}:{next_index}", now.series_id);
                    hongguo_service::play_service::online::resume_fill(
                        &self.state,
                        &next_vid,
                        now.definition,
                        &progress_key,
                    );
                }
                self.save_progress_now();
                if let Some(now) = self.now.as_mut() {
                    now.vid_index = next_index;
                    now.vid = next_vid;
                }
                self.system_switch_started = Some(Instant::now());
                self.system_video_ready = true;
                self.switching = false;
                self.buffering = false;
                self.pending_title = None;
                self.play_error = None;
                self.wake_controls(cx);
                // 刚接力的集已消费旧 pending；下一集预加载要用新 now 重新入队。
                self.system_preload_pending = None;
                if let Some(now) = self.now.clone() {
                    self.load_danmaku(now.vid.clone(), now.series_id.clone(), cx);
                    self.load_episode_counts(&now, cx);
                }
                self.preload_system_next(cx);
                log::info!("[Home] 系统播放器队列接力第 {next_index} 集（未重建播放器）");
                cx.notify();
                return;
            }
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
        // 播放期系统活动断言（无条件持有，直到用户暂停）：无声/被遮挡/后台的
        // 进程会被 macOS 深度节流——轮询被拉长到 20s、渲染循环整个停摆。断言
        // 必须挂在节流打不到的 UI 路径，不能挂 render（被节流的恰恰是 render）。
        crate::play_activity::set_active(true);
        self.switching = true;
        // 新的起播/切集目标会重排预加载；旧的 pending 不挡这一次。
        self.system_preload_pending = None;
        self.system_switch_started = Some(Instant::now());
        // 新一次起播尝试：清上一轮失败态；自动重试记账位不能在这里清——
        // 会「失败→重试→失败」死循环（只在引擎就位时复位）。
        self.play_error = None;
        // 批量补齐推荐位的分集档案与类型，媒体预取由首页共用策略处理。
        let series_window = {
            let items = self.display_items();
            let mut series = Vec::new();
            for item in items
                .iter()
                .skip(self.cursor())
                .filter(|it| !it.vid.is_empty() && it.content_type != 0)
                .take(OFFICIAL_PRELOAD_WINDOW)
            {
                if !series.contains(&item.series_id) {
                    series.push(item.series_id.clone());
                }
            }
            series
        };
        if series_window.len() > 1 {
            let state = Arc::clone(&self.state);
            bridge::spawn(async move {
                let env = state.api_env();
                match hongguo_api::detail::fetch_episode_lists_batch(&series_window, &env).await {
                    Ok(lists) => {
                        let count = lists.len();
                        for list in lists {
                            let sid = list.series_id.clone();
                            let series =
                                hongguo_service::series_service::resolver::series_from_list(list);
                            if let Err(e) = state.store.upsert_series(&series) {
                                log::warn!("[Home] 预取档案落库失败（{sid}）: {e}");
                            }
                        }
                        log::info!("[Home] 批量档案预取 {count} 部落库");
                    }
                    Err(e) => log::warn!("[Home] 批量档案预取失败（忽略）: {e}"),
                }
            });
        }
        // 切剧立刻清「画面已就绪」走新剧封面（不闪上一部残影）；同剧切集/
        // 换清晰度不清——原生画面层还挂着上一集画面，新集首帧事件会接棒。
        let same_series = self
            .now
            .as_ref()
            .is_some_and(|n| n.series_id == item.series_id);
        if !same_series {
            self.system_video_ready = false;
        }
        self.pending_title = Some(if item.title.is_empty() {
            crate::i18n::t("player.preparingShort")
        } else {
            item.title.clone()
        });
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.load_cover_of(&item, cx);
        cx.notify();

        let state = Arc::clone(&self.state);
        let history = self.history.clone().unwrap_or_default();
        // 事件通道 8 深度足够：系统播放器只发低频控制事件，背压照旧作用在发送端。
        let (events, rx) = mpsc::channel::<PlayerEvent>(8);
        let epoch = self.epoch.load(Ordering::Relaxed);
        let epoch_shared = Arc::clone(&self.epoch);
        // 事件循环先于引擎起：Channel 关闭（引擎丢弃 → 发送端清零）即自然退出
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
            let mut deferred: Option<(SystemSpec, super::NowPlaying)> = None;
            let _ = this.update(cx, |view, cx| {
                // 陈旧任务（期间用户又切了剧）：不接管，句柄随 result 丢弃自停
                if view.epoch.load(Ordering::Relaxed) != epoch {
                    return;
                }
                match result {
                    // 视频位是渲染线程私有对象，不能在实体借用内建：open 的 STA
                    // COM 调用会泵窗口消息，泵里重入会撞 HomeView 借用。先把参数
                    // 带出 update，借用解除后再开（下方第二个 update）。
                    Ok((spec, now)) => deferred = Some((spec, now)),
                    Err(error) => view.handle_play_failure(error, cx),
                }
                cx.notify();
            });
            let Some((spec, now)) = deferred else {
                return;
            };
            // 借用已解除：泵消息重入也只是普通的绘制/无障碍路径。
            let opened = crate::platform::open_system_player(
                spec.source,
                spec.start_secs,
                spec.rate,
                spec.events,
                spec.on_stall,
            );
            let _ = this.update(cx, |view, cx| {
                // 开的这几毫秒里用户又切了剧：句柄随 opened 丢弃自停（没有
                // Play 调用不会出声）。
                if view.epoch.load(Ordering::Relaxed) != epoch {
                    return;
                }
                match opened {
                    Some(handle) => view.install_player(handle, now, epoch, cx),
                    None => view.handle_play_failure(
                        hongguo_common::AppError::Media(crate::i18n::t("player.systemUnavailable")),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 把开好的播放器句柄接进视图：偏好重放、落盘收尾、数据面重拉与切换
    /// 态解除。
    fn install_player(
        &mut self,
        handle: Player,
        now: super::NowPlaying,
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        // 偏好重放：换集重建播放器后把音量/静音按存值发给新实例。倍速已随打开
        // 参数传入，只在分歧时补发——同值补发是平白的重复调用。
        if (handle.rate() - self.rate).abs() > 1e-9 {
            handle.set_rate(self.rate);
        }
        handle.set_volume(self.volume);
        handle.set_muted(self.muted);
        // 换播放器前 force 落盘 + 观看历史：旧播放器与旧 now 还在手里、读数
        // 有效；首次起播 player 为空自然跳过。
        self.save_progress_now();
        self.player = Some(handle);
        // 系统播放器没有内容事件流，弹幕推进/进度落盘靠 30fps 节拍。
        self.spawn_system_tick(epoch, cx);
        // 连播锁定跟着目标走：切到别的剧即解除，同剧连播保持。
        if self.binge.as_deref() != Some(now.series_id.as_str()) {
            self.binge = None;
        }
        self.load_intro(now.series_id.clone(), cx);
        // 剧终接力的下一季数据面随起播预取（剧终时通常已就绪）。
        self.load_related_seasons(now.series_id.clone(), cx);
        // 互动数据：集级公开计数（档案）+ mget 最近互动。
        self.load_episode_counts(&now, cx);
        self.load_interact_state(cx);
        // 弹幕按集重拉（社交态不缓存）。必须在 now 就位之后且显式传 series_id：
        // 先于赋值调用会首播拿不到弹幕、换剧错拿上一部的 series_id。
        self.load_danmaku(now.vid.clone(), now.series_id.clone(), cx);
        self.now = Some(now);
        // FirstFrame 可能早于 now 赋值到达；这里补一次预加载。
        self.preload_system_next(cx);
        self.switching = false;
        self.buffering = false;
        self.play_retried = false;
        log::info!("[Home] 播放器就位（epoch={epoch}），switching 解除");
    }

    /// 起播失败收尾：按 retry 档自动重试一次，复发才落可见错误态。
    fn handle_play_failure(&mut self, e: hongguo_common::AppError, cx: &mut Context<Self>) {
        log::warn!("[Home] 起播失败: {e}");
        self.switching = false;
        self.buffering = false;
        // 自动重试一次（换目标/换集失败同样算一次预算），再失败才落错误态。
        if !self.play_retried && !matches!(e, hongguo_common::AppError::PlaybackMaterials(_)) {
            self.play_retried = true;
            log::info!("[Home] 起播失败，自动重试一次");
            if let Some(item) = self.display_items().get(self.cursor()).cloned() {
                self.play_target(item, None, None, cx);
                cx.notify();
                return;
            }
        }
        // 失败必须可见：pending_title 徽标随 switching 消失，必须落可见错误态。
        let message = crate::i18n::app_error_text(&e);
        self.play_error = Some(crate::i18n::tf(
            "common.failedWith",
            &[("message", message.as_str())],
        ));
    }

    /// 切清晰度：同集同位重播，缓存按 (vid, 档位) 分条目——切回去不用重新
    /// 下载，也不动正在播的那一档。`None` = 回到自动（沿用上轮解析到的档位）。
    pub(in crate::home) fn switch_definition(
        &mut self,
        value: Option<u32>,
        cx: &mut Context<Self>,
    ) {
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

    /// 选集浮层里点某一集：跳集，不关浮层（连选连跳）；
    /// 信息流里主动选集 = 锁定本剧连播。
    pub(in crate::home) fn pick_episode(&mut self, vid_index: u32, cx: &mut Context<Self>) {
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

    /// 这一部是否已看完（看到最后一集即算，与片内进度无关）。
    pub(in crate::home) fn series_finished(&self, series_id: &str) -> bool {
        self.history
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|h| h.series_id == series_id)
            .is_some_and(WatchHistoryItem::is_finished)
    }

    /// 跳过已看完的剧：接历史里下一部未看完的（按列表顺序，末尾回绕）；
    /// 都没有就回信息流从当前游标找。绝不 prepare/prefetch 被跳过的剧。
    fn skip_finished_series(&mut self, after: &str, cx: &mut Context<Self>) {
        let items = self.history.clone().unwrap_or_default();
        let start = items
            .iter()
            .position(|h| h.series_id == after)
            .map(|index| index + 1)
            .unwrap_or(0);
        let next = items[start..]
            .iter()
            .chain(items[..start].iter())
            .find(|h| !h.is_finished())
            .cloned();
        if let Some(next) = next {
            let item = StreamItem {
                series_id: next.series_id.clone(),
                title: next.title.clone(),
                cover: next.cover.clone(),
                ..Default::default()
            };
            log::info!(
                "[Home] 已看完《{}》→ 跳过，接下一部未看完《{}》",
                after,
                next.title
            );
            self.binge = Some(next.series_id.clone());
            // 不拿历史行里的集号/秒数硬起播：让起点决策合并本地断点与云端历史。
            self.play_target(item, None, None, cx);
            return;
        }
        log::info!("[Home] 已看完《{after}》→ 历史无未完成项，回信息流");
        self.binge = None;
        self.step(1, cx);
    }

    /// 详情页播放入口（DetailEvent::Play → 壳 → 这里）：明确要追这部，
    /// 锁定连播；起播集与起点由详情页的继续播放决策给定。
    pub(crate) fn play_from_detail(
        &mut self,
        series_id: String,
        vid_index: u32,
        start_secs: f64,
        cx: &mut Context<Self>,
    ) {
        // 已看完的剧不续播、不起播放器、不 prepare/prefetch；直接跳下一部。
        if self.series_finished(&series_id) {
            self.skip_finished_series(&series_id, cx);
            return;
        }
        // 起点复核：列表页调用方只拿得到本地档案，云端历史 >3 秒时用云端位置
        // 兜底——传上来的 (1, 0.0) 即「本地没有可续播记录」的形态；显式续播/
        // 换集不受影响。
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
            let info = tokio::task::spawn_blocking(move || {
                state
                    .store
                    .series_by_id(&sid)
                    .ok()
                    .flatten()
                    .map(|s| (s.title, s.cover))
            })
            .await
            .map_err(|e| AppError::Internal(format!("剧集档案阻塞池任务失败: {e}")))?;
            Ok::<_, AppError>(info)
        });
        cx.spawn(async move |this, cx| {
            let info = match done.await.expect("tokio 桥不会丢弃发送端") {
                Ok(info) => info,
                Err(e) => {
                    log::warn!("[Home] 剧集档案读取失败: {e}");
                    None
                }
            };
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
                    content_type: 0,
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

    /// 离开首页的卸载清理：强落进度、释放引擎、收起浮层——HomeView 实体常驻
    /// 带来缓存不丢，但停播面必须在这里补齐（否则切走后解码/声音/连播/落盘
    /// 照常跑）。回流态不动：回首页由 [`Self::resume_if_idle`] 接上。
    pub(crate) fn release_for_leave(&mut self, cx: &mut Context<Self>) {
        if self.player.is_none() {
            return;
        }
        // 先落进度再放引擎：读数来自句柄，Drop 之后就是 0 了。
        self.save_progress_now();
        // 播放期活动断言随引擎一起释放：断言只在用户手动暂停时收，离开不收
        // 的话壳不在前台也照样挡着机器休眠。
        crate::play_activity::set_active(false);
        // 陈旧事件与在途起播任务按纪元弃置；句柄 Drop 即任务 abort、音频设备
        // 释放、事件通道关闭（循环自灭）。
        if let Some(player) = &self.player {
            player.detach();
        }
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.player = None;
        self.system_video_ready = false;
        // 离开首页也推进代次：旧播放器的 Ended/尾部事件不能再推进任何集。
        self.play_generation = self.play_generation.wrapping_add(1);
        self.system_preload_pending = None;
        hongguo_stream::cache().clear_next();
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

    /// 回到首页的续播：引擎不在（被 [`Self::release_for_leave`] 释放 /
    /// 冷启动还没起）就按当前游标重走起点决策——从落盘进度接上。信息流
    /// 还没就位时不动，首批落定的冷启动钩子会接管。
    pub(crate) fn resume_if_idle(&mut self, cx: &mut Context<Self>) {
        // 回首页的过期换一批：流数据 >5 分钟就重开会话取新一批——在播剧由
        // pin_hold 钉在最前继续播，滚下去才是新一批；无在播目标（冷启动）不换。
        if self.now.is_some() && self.feed_stale() {
            log::info!("[Home] 回首页：流已过期（>5 分钟），换一批（在播不动）");
            self.pin_hold = true;
            self.load_feed(self.active.clone(), true, cx);
            // 刷新清列表并把游标归零：把游标对回「在播的那部」在展示列表里的
            // 位置（已被换出则 pin 在最前，通常即 0）。
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
        self.controls_until = Some(Instant::now() + Self::CONTROLS_VISIBLE);
        self.play_target(item, None, None, cx);
    }
}
