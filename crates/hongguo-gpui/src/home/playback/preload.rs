//! 预取与装饰数据：系统播放器下一集预加载、周期节拍、后续部流预取、
//! 相邻剧档案预取、简介与追更态。

use super::*;

impl super::HomeView {
    /// 预先给系统播放器塞入下一集（AVQueuePlayer）：先把下一集流注册好
    ///（不在切集时才发现没注册、被迫重建播放器），再挂 AVPlayerItem。
    pub(in crate::home) fn preload_system_next(&mut self, cx: &mut Context<Self>) {
        if !crate::video_backend::is_system() {
            return;
        }
        let Some(now) = self.now.as_ref() else { return };
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let (series_id, definition, next_index) =
            (now.series_id.clone(), now.definition, now.vid_index + 1);
        let Ok(Some(series)) = self.state.store.series_by_id(&series_id) else {
            return;
        };
        let Some(episode) = series
            .episodes
            .iter()
            .find(|episode| episode.vid_index == next_index)
        else {
            return;
        };
        let vid = episode.vid.clone();
        if vid.is_empty() || self.system_preload_pending.as_deref() == Some(vid.as_str()) {
            return;
        }
        self.system_preload_pending = Some(vid.clone());
        // 先在主线程同步落「下一集」标记：连播预取可能已经开跑，晚一步它
        // 就会在 8MB 预算处收工，再由 ensure 唤醒已经来不及。
        hongguo_stream::cache().mark_next(&vid, definition);
        let player = Arc::clone(player);
        let state = Arc::clone(&self.state);
        let progress_key = format!("{series_id}:{next_index}");
        let ready_vid = vid.clone();
        let ready_key = progress_key.clone();
        let done = bridge::spawn(async move {
            ensure_stream_ready(&state, &ready_vid, definition, &ready_key).await
        });
        cx.spawn(async move |this, cx| {
            let ready = done.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if view.system_preload_pending.as_deref() == Some(vid.as_str()) {
                    view.system_preload_pending = None;
                }
                let still_next = view.now.as_ref().is_some_and(|now| {
                    now.series_id == series_id && now.vid_index + 1 == next_index
                });
                if !still_next {
                    return;
                }
                if !ready {
                    log::warn!("[Home] 第 {next_index} 集流 5s 内未注册，预加载继续由 loader 等待");
                }
                let stall_state = Arc::clone(&view.state);
                let stall_vid = vid.clone();
                let stall_key = progress_key.clone();
                let on_stall: Option<super::player::StallHook> = Some(Arc::new(move || {
                    resume_fill(&stall_state, &stall_vid, definition, &stall_key);
                }));
                if player.preload_next(&vid, definition, on_stall) {
                    log::info!(
                        "[Home] 已预加载第 {next_index} 集（{vid}@{definition}，流已注册={ready}）"
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 系统播放器的周期节拍：没有 Frame 事件，弹幕推进/控制条刷新/进度落盘
    /// 改由 30fps 定时器驱动（Tauri `<video>` 的 timeupdate 同语义）。
    pub(in crate::home) fn spawn_system_tick(&mut self, epoch: u64, cx: &mut Context<Self>) {
        // 无人值守启动时进程可能不在前台，GPUI 会停止请求帧（视频层也就
        // 没有第一次 render 去挂载）。系统后端播放时把窗口提到前台一次；
        // 用户手动启动时本来就是前台，无副作用。
        let _ = self
            .window_handle
            .update(cx, |_, window, _| window.activate_window());
        cx.spawn(async move |this, cx| {
            loop {
                // 这一拍只做逻辑（入场/回收/重锚/进度落盘），**不推进位置**：
                // 弹幕位置是挂钟的纯函数，重绘由弹幕层的 vsync 动画驱动（见
                // `stage/render.rs`）。频率与视频 fps / 刷新率都无关。
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let keep = this
                    .update(cx, |view, cx| {
                        if view.epoch.load(Ordering::Relaxed) != epoch {
                            return false;
                        }
                        if !view.player.as_ref().is_some_and(|p| p.is_system()) {
                            return false;
                        }
                        view.danmaku_tick(cx);
                        view.save_progress_maybe();
                        // 有弹幕动画在跑时**不要**再 notify：动画本身每合成帧
                        // 都会重绘，这里的 notify 会打断它的帧节奏（顿挫的另一
                        // 半原因）。没有动画驱动时才需要它维持进度条等刷新。
                        let animating = view.danmaku_on
                            && !view.danmaku_flights.is_empty()
                            && !view.player.as_ref().is_some_and(|p| p.is_paused());
                        if !animating {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
        .detach();
    }

    /// 首帧出画：预取信息流后续 B 部的流（每部一次；Tauri 首帧发令同款）。
    /// B = `Settings.prefetch_episodes`（0=全关，含换剧预取）——快滑时
    /// 每一部都有头部缓存可接，不再是只有紧邻下一部热、其余全冷启动。
    pub(in crate::home) fn prefetch_next_stream(&mut self, cx: &mut Context<Self>) {
        let breadth = self.state.settings().prefetch_episodes;
        if breadth == 0 {
            return;
        }
        let items = self.display_items();
        let cursor = self.cursor();
        let mut launched = 0usize;
        for offset in 1..=breadth as usize {
            let Some(next) = items.get(cursor + offset).cloned() else {
                break;
            };
            // 已看完的后续剧不预取、不缓存、不加载。
            if self.series_finished(&next.series_id) {
                continue;
            }
            if next.series_id.is_empty() || !self.stream_prefetched.insert(next.series_id.clone()) {
                continue;
            }
            launched += 1;
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
                    // 失败不算数：下次再成为「后续部」时允许重试
                    let _ = this.update(cx, |view, _| {
                        view.stream_prefetched.remove(&next.series_id);
                    });
                    log::info!("[Home] 预取后续部流失败（忽略）: {e}");
                }
            })
            .detach();
        }
        if launched > 0 {
            log::info!("[Home] 沉浸流预取铺 {launched} 部");
        }
    }

    async fn prefetch_stream_task(
        state: Arc<AppStateInner>,
        series_id: String,
        history: Vec<WatchHistoryItem>,
    ) -> AppResult<()> {
        if history
            .iter()
            .find(|h| h.series_id == series_id)
            .is_some_and(WatchHistoryItem::is_finished)
        {
            log::info!("[Home] 预取跳过已看完《{series_id}》");
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
        // 每部只铺入口集（头部 8MB 有界，升格当前集才无界续填）；同剧
        // 后续集的连播铺集由起播的 schedule_following_episodes 负责，
        // 这里不重复——铺多了就是把带宽从当前集嘴里抢走
        hongguo_service::play_service::online::schedule_next_episode_prefetch(
            state, &series_id, entry,
        );
        Ok(())
    }

    /// 相邻剧分集档案预取（换剧时 resolve 秒回）。
    pub(in crate::home) fn prefetch_neighbor_archives(&mut self) {
        let items = self.display_items();
        let cursor = self.cursor();
        for idx in [cursor + 1, cursor.saturating_sub(1)] {
            let Some(item) = items.get(idx) else {
                continue;
            };
            let sid = item.series_id.clone();
            if sid.is_empty()
                || self.series_finished(&sid)
                || !self.archives_prefetched.insert(sid.clone())
            {
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

    /// 拉当前剧的简介、全剧收藏数与连载状态（video_detail 的 series_intro /
    /// followed_cnt / series_status，Tauri useSeriesDetailMeta 同源）：装饰性
    /// 信息，失败静默降级、允许重试；每剧只拉一次（会话内缓存，Tauri
    /// staleTime 10 分钟同语义）。followed_cnt 是互动栏「收藏」键的计数源；
    /// series_status 决定「追更」键显隐。
    pub(in crate::home) fn load_intro(&mut self, series_id: String, cx: &mut Context<Self>) {
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
}
