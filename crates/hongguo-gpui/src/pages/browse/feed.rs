//! 找剧页浏览流数据：筛选面板首拉、浏览流翻页、下载角标快照、封面懒加载
//! 与起播入口。

use super::*;

impl super::BrowseView {
    /// 进入页面：拉筛选面板 + 首屏浏览流 + 下载角标快照。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.refresh_downloaded(cx);
        if self.panel_rows.is_none() && !self.panel_failed {
            let state = Arc::clone(&self.state);
            let rx = bridge::spawn(async move {
                let env = state.api_env();
                hongguo_api::discover::fetch_browse_panel(&env).await
            });
            cx.spawn(async move |this, cx| {
                let result = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    match result {
                        Ok(rows) => view.panel_rows = Some(rows),
                        Err(e) => {
                            log::warn!("[Browse] 筛选面板失败: {e}");
                            view.panel_failed = true;
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if self.feed_items.is_empty() && !self.feed_loading {
            self.load_browse_page(cx);
        }
    }

    /// 下载角标：按剧聚合已完成任务数（本地快照，直读即刷）。
    fn refresh_downloaded(&mut self, cx: &mut Context<Self>) {
        let mut map: HashMap<String, u32> = HashMap::new();
        for task in self.state.queue().all() {
            if task.status == hongguo_common::model::TaskStatus::Completed {
                *map.entry(task.series_id).or_insert(0) += 1;
            }
        }
        self.downloaded = map;
        let _ = cx;
    }

    /// 拉一页浏览流（空 offset = 重开/换筛选；带游标 = 翻页）。
    pub(super) fn load_browse_page(&mut self, cx: &mut Context<Self>) {
        if self.feed_loading {
            return;
        }
        self.feed_loading = true;
        self.feed_failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let filters = self.filters.clone();
        let offset = self.feed_next_offset;
        let session_id = self.feed_session_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::discover::fetch_browse(&filters, offset, &session_id, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.feed_loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[Browse] GPUI 壳 -> {} 条", page.items.len());
                        if offset == 0 {
                            // 首页整替（重开/换筛选/退出搜索）：虚拟列表按整替
                            // 对账（reset，滚动回顶）
                            view.grid_rows.replace();
                            view.feed_items = page.items.clone();
                        } else {
                            view.feed_items.extend(page.items.clone());
                        }
                        view.feed_next_offset = page.next_offset;
                        view.feed_session_id = page.session_id.clone();
                        view.feed_has_more = page.has_more;
                    }
                    Err(e) => {
                        log::warn!("[Browse] 浏览流失败: {e}");
                        view.feed_failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 重新按当前筛选拉首页（切筛选/退出搜索回浏览）。
    pub(super) fn reload_browse(&mut self, cx: &mut Context<Self>) {
        self.feed_next_offset = 0;
        self.feed_session_id.clear();
        self.feed_has_more = false;
        self.load_browse_page(cx);
    }

    pub(super) fn play_series(&self, series_id: String, cx: &mut Context<Self>) {
        let (vid_index, start_secs) = hongguo_service::play_service::resume::resolve_entry(
            &self.state.store,
            &series_id,
            &[],
        )
        .ok()
        .unwrap_or((1, 0.0));
        cx.emit(BrowseEvent::Play {
            series_id,
            vid_index,
            start_secs,
        });
    }
}
