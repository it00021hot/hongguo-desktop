//! 详情页数据装载：档案/meta/观看历史/互动态/相关推荐预约态、图片缓存与
//! 未上线档案合成。

use super::*;

impl DetailView {
    /// 装载一部剧（同剧已装载则跳过；换剧强制重载）。档案与装饰性数据
    /// （meta/剧评/相关推荐/互动态）并行各拉各的，互不拦页面——Tauri
    /// react-query 各 query 独立降级同口径。
    pub(crate) fn load(&mut self, series_id: String, cx: &mut Context<Self>) {
        self.load_with_prefill(series_id, None, cx);
    }

    /// 装载（可带来源页 prefill）：解析失败且有 prefill 时走「即将上线」
    /// 降级视图（prefill 合成 0 集档案，正常渲染流只换动作行）。
    pub(crate) fn load_with_prefill(
        &mut self,
        series_id: String,
        prefill: Option<DetailPrefill>,
        cx: &mut Context<Self>,
    ) {
        if self.loading || (self.series_id == series_id && self.series.is_some()) {
            return;
        }
        self.series_id = series_id.clone();
        self.series = None;
        self.meta = None;
        self.load_failed = false;
        self.upcoming = false;
        self.reserved = prefill.as_ref().map(|p| p.reserved).unwrap_or(false);
        self.reserve_pending = false;
        self.prefill = prefill.clone();
        self.tab = DetailTab::Episodes;
        self.reviews = Arc::new(Vec::new());
        self.reviews_total = 0;
        self.reviews_score.clear();
        self.reviews_score_cnt = 0;
        self.reviews_tag_stats.clear();
        self.reviews_cursor.clear();
        self.reviews_has_more = false;
        self.related = None;
        self.related_reserved.clear();
        self.related_reserve_pending.clear();
        self.related_reserve_loaded = false;
        self.interact_items = Arc::new(Vec::new());
        self.collect_pending = false;
        self.digg_pending = false;
        self.follow_is_on = None;
        self.follow_pending = false;
        self.history.clear();
        self.review_stars = 0;
        self.review_hover_stars = 0;
        self.review_emoji_open = false;
        self.review_sending = false;
        self.review_submit_pending = false;
        // 换剧清空输入框（懒建之后才有效；值随渲染期 set_value 落）
        self.review_clear_input = self.review_input.is_some();
        // 剧评互动状态随换剧清零
        self.review_digg_override.clear();
        self.review_replies.clear();
        self.review_replies_cursor.clear();
        self.review_replies_open.clear();
        self.review_replies_loading.clear();
        self.review_reply_target = None;
        self.review_reply_sending = false;
        self.review_reply_submit_pending = false;
        self.review_reply_clear = self.review_reply_input.is_some();
        self.review_deleting = false;
        self.loading = true;
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let cached = state
                .store
                .series_by_id(&sid)?
                .filter(|s| !s.episodes.is_empty());
            // 未上线剧解析必然失败：有 prefill 就用它合成 0 集档案
            let (series, upcoming) = match cached {
                Some(s) => (s, false),
                None => {
                    match hongguo_service::series_service::resolver::resolve_series(&sid, &env)
                        .await
                    {
                        Ok(s) => {
                            state.store.upsert_series(&s)?;
                            (s, false)
                        }
                        Err(e) => match prefill.clone() {
                            Some(p) => (synthesize_upcoming_series(&sid, &p), true),
                            None => return Err(e),
                        },
                    }
                }
            };
            // 装饰性 meta 失败不拦页面（头部相应行不渲染，Tauri 同口径）
            let meta = hongguo_api::detail::fetch_series_meta(&sid, &env)
                .await
                .ok();
            Ok::<_, hongguo_common::AppError>((series, meta, upcoming))
        });
        // 云端观看历史（「继续看」兜底源；登录才有数据，匿名回空表）——
        // 与档案并行各拉各的，晚到只是一拍刷新按钮文案
        let state_history = Arc::clone(&self.state);
        let rx_history = bridge::spawn(async move {
            let env = state_history.api_env();
            hongguo_api::history::fetch_watch_history(0, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx_history.await.expect("tokio 桥不会丢弃发送端");
            if let Ok(page) = result {
                let _ = this.update(cx, |view, cx| {
                    view.history = page.items;
                    cx.notify();
                });
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok((series, meta, upcoming)) => {
                        view.load_image(series.cover.clone(), cx);
                        // 出品方/演职员头像随 meta 预取（头部芯片用；先收集
                        // URL 再改 view，避免同时借 self.meta）
                        let creator_avatars: Vec<String> = meta
                            .as_ref()
                            .map(|m| {
                                m.creators
                                    .iter()
                                    .map(|c| c.avatar.clone())
                                    .filter(|a| !a.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        view.meta = meta;
                        for url in creator_avatars {
                            view.load_image(url, cx);
                        }
                        view.series = Some(Arc::new(series));
                        view.upcoming = upcoming;
                        if !upcoming {
                            // 头部统计行的评分来自剧评分页首页 extra——剧评
                            // 本来就要拉，这里一并驱动
                            view.load_reviews_first_page(cx);
                            view.load_related(cx);
                            view.load_interact_state(cx);
                            view.load_follow_state(cx);
                        }
                    }
                    Err(e) => {
                        log::warn!("[Detail] {series_id} 档案装载失败: {e}");
                        view.load_failed = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 相关推荐卡的预约态：预约列表（离线 + 在线两个 tab，Tauri ReserveButton
    /// 两处都查同口径）拉一次；未登录不拉（按钮点击走登录回执）。
    pub(super) fn load_related_reservations(&mut self, cx: &mut Context<Self>) {
        if self.related_reserve_loaded || !self.logged_in() {
            return;
        }
        self.related_reserve_loaded = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let offline = hongguo_api::reservation::fetch_reservations(false, &env).await;
            let online = hongguo_api::reservation::fetch_reservations(true, &env).await;
            let mut ids = std::collections::HashSet::new();
            for page in [offline, online].into_iter().flatten() {
                for item in page.items {
                    ids.insert(item.series_id);
                }
            }
            ids
        });
        cx.spawn(async move |this, cx| {
            let ids = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.related_reserved.extend(ids);
                cx.notify();
            });
        })
        .detach();
    }

    /// 登录与否（settings 里有账号即已登录，与首页互动栏同口径）。
    pub(super) fn logged_in(&self) -> bool {
        self.state
            .settings()
            .account
            .map(|a| a.user_id)
            .is_some_and(|id| !id.is_empty())
    }

    /// 互动回显（收藏/点赞态）：mget 全量列表，best-effort（失败留空 =
    /// 全按未互动，Tauri useInteractionState 同口径）。登录后才拉。
    pub(super) fn load_interact_state(&mut self, cx: &mut Context<Self>) {
        if !self.logged_in() {
            return;
        }
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::interact::fetch_interaction_state(&env)
                .await
                .map(|s| s.items)
                .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let items = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                view.interact_items = Arc::new(items);
            });
        })
        .detach();
    }

    /// 懒加载一张远端图进 images 表（封面/头像/相关卡共用；管线与全屏
    /// 占位同源，状态各自持有）。
    pub(super) fn load_image(&mut self, url: String, cx: &mut Context<Self>) {
        if url.is_empty()
            || self.images.contains_key(&url)
            || !self.images_pending.insert(url.clone())
        {
            return;
        }
        let rx = cover_img::fetch(url.clone());
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match bytes {
                Ok(b) => match image::load_from_memory(&b) {
                    Ok(decoded) => {
                        view.images.insert(
                            url.clone(),
                            Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                        );
                        cx.notify();
                    }
                    Err(e) => {
                        log::warn!("[Detail] 图片解码失败: {e}");
                        view.images_pending.remove(&url);
                    }
                },
                Err(e) => {
                    log::warn!("[Detail] 图片加载失败: {e}");
                    view.images_pending.remove(&url);
                }
            });
        })
        .detach();
    }
}

/// prefill → 0 集合成档案（未上线剧）。只填展示必需字段；分集为空，
/// 选集 tab 自然落「暂无分集信息」。
pub(super) fn synthesize_upcoming_series(series_id: &str, prefill: &DetailPrefill) -> Series {
    Series {
        series_id: series_id.to_string(),
        title: prefill.title.clone(),
        cover: prefill.cover.clone(),
        episode_count: 0,
        followed_cnt: 0,
        tags: prefill.tags.clone(),
        episodes: Vec::new(),
        dismissed: false,
    }
}
