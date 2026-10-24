//! 历史列表数据：双模式加载、封面、tab/搜索过滤与预览种子。

use super::*;

/// 本地条目 → 渲染行（字段与云端同形；u32 集号升为 i64 统一口径）。
fn row_from_local(item: &LocalHistoryItem) -> WatchHistoryItem {
    WatchHistoryItem {
        series_id: item.series_id.clone(),
        title: item.title.clone(),
        cover: item.cover.clone(),
        vid_index: i64::from(item.vid_index),
        vid: item.vid.clone(),
        position_ms: item.position_ms,
        duration_ms: item.duration_ms,
        episode_cnt: i64::from(item.episode_cnt),
        updated_at_ms: item.updated_at_ms,
    }
}

impl HistoryView {
    /// 拉历史（双模式，Tauri 同款）：登录 = 云端首屏一页；未登录 = 本地
    /// 观看记录（播放时已落库，全量读）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        if self.items.is_empty() {
            self.phase = Phase::Loading;
        }
        cx.notify();
        if !self.logged_in() {
            // 本地模式（issue #10）
            let state = Arc::clone(&self.state);
            let rx = bridge::spawn(async move {
                tokio::task::spawn_blocking(move || state.store.local_history_list())
                    .await
                    .expect("阻塞池 join")
            });
            cx.spawn(async move |this, cx| {
                let result = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.loading = false;
                    match result {
                        Ok(items) => {
                            log::info!("[History] GPUI 壳本地模式 -> {} 条", items.len());
                            view.items = items.iter().map(row_from_local).collect();
                            view.selected.clear();
                            view.phase = Phase::Loaded;
                        }
                        Err(e) => {
                            log::warn!("[History] GPUI 壳本地读取失败: {e}");
                            if view.items.is_empty() {
                                view.phase = Phase::Failed(crate::i18n::app_error_text(&e));
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::history::fetch_watch_history(0, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[History] GPUI 壳 -> {} 条", page.items.len());
                        view.items = page.items;
                        view.selected.clear();
                        view.phase = Phase::Loaded;
                    }
                    Err(e) => {
                        log::warn!("[History] GPUI 壳失败: {e}");
                        if view.items.is_empty() {
                            view.phase = Phase::Failed(crate::i18n::app_error_text(&e));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 过滤（tab + 标题/seriesId 包含，Tauri 同口径）。
    ///
    /// 返回 `self.items` 的下标：虚拟列表每帧都要重建条目数，按索引走免得
    /// 每帧克隆整张表。
    pub(super) fn visible_indices(&self, cx: &App) -> Vec<usize> {
        let q = self.search.read(cx).value().trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                match self.tab {
                    HistoryTab::Finished if !item.is_finished() => return false,
                    HistoryTab::Unfinished if item.is_finished() => return false,
                    _ => {}
                }
                if q.is_empty() {
                    return true;
                }
                item.title.to_lowercase().contains(&q) || item.series_id.to_lowercase().contains(&q)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// 预览种子（screenshot feature）：灌假历史行，跳过网络。
    /// `history-empty` = 空态；`history-manage` = 管理模式（前两条已选）；
    /// `history-local` = 未登录本地模式（顶部提示行）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "history-empty";
        // 本地模式场景之外都按登录态出图（本地场景留给提示行形态）
        self.preview_logged_in = scene != "history-local";
        self.phase = Phase::Loaded;
        self.loading = false;
        self.items = if empty {
            Vec::new()
        } else {
            crate::preview::fixtures::watch_history_items()
        };
        for (i, item) in self.items.iter_mut().enumerate() {
            let url = format!("preview://history/{i}");
            item.cover = url.clone();
            crate::preview::fixtures::put_preview_image(url, crate::preview::fixtures::cover(i));
        }
        if scene == "history-manage" {
            self.managing = true;
            self.selected = self
                .items
                .iter()
                .take(2)
                .map(|i| i.series_id.clone())
                .collect();
        }
        cx.notify();
    }
}
