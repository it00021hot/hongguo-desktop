//! 切剧/切集编排：滚轮与连播切换、起播目标、切片与选集、已看完跳过，
//! 以及离开首页的释放与回首页续播。

use super::*;

use super::open::{Opened, PlayRequest};

impl super::HomeView {
    /// 连播锁定内的切集（滚轮/↑↓，Tauri useBingeRelay 的 stepEpisode 同款）：
    /// 越过第 1 集原地不动；滚过末集 = 剧终，走剧终接力（下一季 → 信息流
    /// 下一条推荐）。
    pub(crate) fn step_episode(&mut self, dir: i32, cx: &mut Context<Self>) {
        let Some(now) = self.now.clone() else {
            return;
        };
        let next = now.vid_index as i64 + dir as i64;
        if next < 1 {
            return;
        }
        if next as usize > now.episode_count {
            // 滚过末集也算剧终（Tauri Ns()→hc() 同款）
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
            // Tauri 同款：滚轮也是「用户在场」，切集时唤醒悬浮层
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

    /// 滚轮冷却裁决 + 记账：到点可切返回 true 并落章。只由滚轮路径调用    /// （键盘 ↑↓ 不吃这把锁）；换剧与连播切集共用——冷却原写在换剧分支
    /// 里，连播切集漏网，一次惯性滚动连跳好几集。
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

    /// 切到目标剧。旧的引擎继续播到新引擎就位（对齐 Tauri「store 目标
    /// 变了但画面还在上一部」的切换语义），就位即整体换装。
    /// `start_override` 给切清晰度保进度用（None = 走起点决策）。
    pub(in crate::home) fn play_target(
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
        //（重建要走取流表+asset+loader，实测首帧 650-970ms；接力 ~70ms）。
        if crate::video_backend::is_system()
            && start_override.is_none()
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
            let next_vid = self
                .state
                .store
                .series_by_id(&item.series_id)
                .ok()
                .flatten()
                .and_then(|series| {
                    series
                        .episodes
                        .into_iter()
                        .find(|episode| episode.vid_index == next_index)
                })
                .map(|episode| episode.vid);
            if let (Some(next_vid), Some(player)) = (next_vid, self.player.as_ref())
                && !next_vid.is_empty()
                && player.advance_preloaded(&next_vid)
            {
                log::info!(
                    "[Home][尾进] 接力 gen={} ep {}→{next_index}",
                    self.play_generation,
                    self.now.as_ref().map(|now| now.vid_index).unwrap_or(0)
                );
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
                // 刚接力的这一集已经消费掉旧 pending；下一集的预加载要
                // 用新的 now 重新入队，不能被旧请求的 pending 挡住。
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
        // 播放期系统活动断言（无条件持有，直到用户暂停）：无声/被遮挡/
        // 后台的进程会被 macOS 深度节流——换集起播的 50ms 轮询实测被拉长
        // 到 20s、GPUI 渲染循环整个停摆。断言必须挂在**节流打不到的入口**
        // （用户/连播事件的 UI 路径），不能挂在 render——被节流的恰恰是
        // render，翻沿永远走不到。暂停即释放（用户在场，恢复由
        // toggle_pause 重新持有）。
        crate::play_activity::set_active(true);
        // 上屏节奏取证换窗口：上一集的节奏/停顿不能算进新一集的起播预滚
        self.paint_diag.reset();
        self.switching = true;
        // 新的起播/切集目标会重排预加载；旧的 pending 不挡这一次。
        self.system_preload_pending = None;
        if crate::video_backend::is_system() {
            self.system_switch_started = Some(Instant::now());
        }
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
        // 旧帧退场（Tauri use-frame-hold 同款真值表）：
        // - 切剧：立刻走新剧封面，上一部的残帧绝不许跨剧泄漏；
        // - 同剧切集/换清晰度：保留上一集末帧顶住画面（`hold_last_frame`），
        //   新集首帧出画即接棒——这才是「切集不闪封面」。
        let same_series = self
            .now
            .as_ref()
            .is_some_and(|n| n.series_id == item.series_id);
        if !same_series {
            self.frame = None;
            self.video_surface.clear();
            self.system_video_ready = false;
        }
        self.hold_last_frame = same_series && self.frame.is_some();
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
        // 事件通道容量**必须小**：Frame 事件各背一帧（1080p NV12
        // CVPixelBuffer ≈ 3MB），64 深度在宿主一时跟不上时就压着近 200MB
        // 的 IOSurface 不还（解码器的输出池也跟着膨胀）。8 帧 ≈ 0.27s 的
        // 内容量，足够吸收正常抖动；更深只会把「晚显示」变成「内存涨」。
        // 宿主侧另有 batch 合并，控制事件（Buffering/Ended/…）依旧按序处理。
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
            let _ = this.update(cx, |view, cx| {
                // 陈旧任务（期间用户又切了剧）：不接管，引擎随 result 丢弃自停
                if view.epoch.load(Ordering::Relaxed) != epoch {
                    return;
                }
                // 系统后端的播放器只能在主线程创建（DComp 视频位归渲染线程）。
                let opened = match result {
                    Ok(Opened::Ready(handle, now)) => Ok((handle, now)),
                    Ok(Opened::System(spec, now)) => crate::platform::open_system_player(
                        spec.source,
                        spec.start_secs,
                        spec.rate,
                        spec.events,
                        spec.on_stall,
                    )
                    .map(|handle| (handle, now))
                    .ok_or_else(|| {
                        hongguo_common::AppError::Media(crate::i18n::t("player.systemUnavailable"))
                    }),
                    Err(error) => Err(error),
                };
                match opened {
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
                        let system_backend = handle.is_system();
                        view.player = Some(handle);
                        if system_backend {
                            view.spawn_system_tick(epoch, cx);
                        }
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
                        if system_backend {
                            // FirstFrame 可能早于 now 赋值到达；这里补一次预加载。
                            view.preload_system_next(cx);
                        }
                        view.switching = false;
                        view.buffering = false;
                        view.play_retried = false;
                        log::info!("[Home] 引擎就位（epoch={epoch}），switching 解除");
                    }
                    Err(e) => {
                        // 系统播放器起不来 = 后端降级，不是播放失败：翻掉开关后用
                        // 自研上屏重走一次起播（不消耗 play_retried 预算）。
                        if crate::video_backend::fall_back_to_gpui() {
                            log::warn!("[Home] 系统播放器不可用，后端降级为 gpui 重起：{e}");
                            view.switching = false;
                            if let Some(item) = view.display_items().get(view.cursor()).cloned() {
                                view.play_target(item, None, None, cx);
                                cx.notify();
                                return;
                            }
                        }
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
                        let message = crate::i18n::app_error_text(&e);
                        view.play_error = Some(crate::i18n::tf(
                            "common.failedWith",
                            &[("message", message.as_str())],
                        ));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 切清晰度（Tauri onDefinitionChange 同款）：同集同位重播，缓存按
    /// (vid, 档位) 分条目——切回去不用重新下载，也不动正在播的那一档。
    /// `None` = 回到自动（沿用上轮解析到的档位）。
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

    /// 选集浮层里点某一集：跳集，**不关浮层**（hgplayer 同款连选连跳）；
    /// 信息流里主动选集 = 锁定本剧连播（Tauri onPickEpisode 同款）。
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
    /// 都没有就回信息流从当前游标找下一部未完成的。绝不 prepare/prefetch
    /// 被跳过的剧，也不留播放器/加载态。
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
            // 不拿历史行里的集号/秒数硬起播：让起点决策合并本地断点与云端
            // 历史（本地 5 秒一写更新），并继续走 clamp。
            self.play_target(item, None, None, cx);
            return;
        }
        log::info!("[Home] 已看完《{after}》→ 历史无未完成项，回信息流");
        self.binge = None;
        self.step(1, cx);
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
        // 已看完的剧不续播、不起播放器、不 prepare/prefetch；直接跳下一部。
        if self.series_finished(&series_id) {
            self.skip_finished_series(&series_id, cx);
            return;
        }
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
        // 离开首页不留残帧（回首页走起点决策重起引擎，首帧到了再上屏；
        // 定格帧是「同剧切集」专属，离开时一并落下）
        self.frame = None;
        self.video_surface.clear();
        self.hold_last_frame = false;
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
}
