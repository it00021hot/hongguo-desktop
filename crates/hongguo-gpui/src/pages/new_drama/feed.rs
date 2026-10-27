use super::*;

use gpui_kit::prelude::*;

use crate::bridge;

impl NewDramaView {
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.refresh_downloaded();
        if self.view == ViewTab::Recommend && self.items.is_empty() && !self.loading {
            self.load_recommend(false, cx);
        } else if self.view == ViewTab::Calendar && !self.cal_loaded {
            self.load_calendar(None, false, cx);
        }
    }

    fn refresh_downloaded(&mut self) {
        let mut map: HashMap<String, u32> = HashMap::new();
        for task in self.state.queue().all() {
            if task.status == hongguo_common::model::TaskStatus::Completed {
                *map.entry(task.series_id).or_insert(0) += 1;
            }
        }
        self.downloaded = map;
    }

    /// 拉一页推荐（`more` = 翻页）。
    ///
    /// 响应归属**发起请求时的频道**：还在展示就整替/拼接展示态，人已切走
    /// 就并回该频道的按键缓存——切频道期间旧列表保持可见（keepPreviousData）。
    pub(super) fn load_recommend(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let gender = self.gender;
        let offset = if more { self.next_offset } else { 0 };
        let session = if more {
            self.session_id.clone()
        } else {
            String::new()
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::new_drama::fetch_new_drama(
                gender,
                offset,
                (!session.is_empty()).then_some(session.as_str()),
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[New] GPUI 壳 -> {} 条", page.items.len());
                        if view.gender == gender {
                            // 首页整替（含 needs_reload 补发）：虚拟列表按整替对账
                            if !more {
                                view.rec_rows.replace();
                            }
                            if more {
                                view.items.extend(page.items.clone());
                            } else {
                                view.items = page.items.clone();
                            }
                            view.next_offset = page.next_offset;
                            view.session_id = page.session_id.clone();
                            view.has_more = page.has_more;
                        } else {
                            let cached = view.gender_pages.entry(gender).or_default();
                            if more {
                                cached.items.extend(page.items.clone());
                            } else {
                                cached.items = page.items.clone();
                            }
                            cached.next_offset = page.next_offset;
                            cached.session_id = page.session_id.clone();
                            cached.has_more = page.has_more;
                        }
                    }
                    Err(e) => {
                        log::warn!("[New] 推荐流失败: {e}");
                        // 有旧列表在手（keepPreviousData 展示中）就保留展示，
                        // 真没数据才给失败态。
                        if view.gender == gender && view.items.is_empty() {
                            view.failed = true;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 拉一页日历（`date` 为空 = 默认日；`more` = 当日续页）。
    pub(super) fn load_calendar(
        &mut self,
        date: Option<String>,
        more: bool,
        cx: &mut Context<Self>,
    ) {
        if self.cal_loading {
            return;
        }
        self.cal_loading = true;
        self.cal_failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let offset = if more { self.cal_next_offset } else { 0 };
        let session = if more {
            self.cal_session_id.clone()
        } else {
            String::new()
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let cursor = (offset > 0).then_some((offset, session.as_str()));
            hongguo_api::calendar::fetch_new_calendar(date.as_deref(), cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.cal_loading = false;
                view.cal_loaded = true;
                match result {
                    Ok(page) => {
                        log::info!("[New] 日历 -> {} 条", page.items.len());
                        // 首页整替：虚拟列表按整替对账（reset，滚动回顶）
                        if !more {
                            view.cal_rows.replace();
                        }
                        if !more {
                            view.cal_items = page.items.clone();
                            if !page.dates.is_empty() {
                                view.cal_dates = page.dates.clone();
                            }
                            if view.cal_selected.is_empty() {
                                view.cal_selected = if page.default_date.is_empty() {
                                    view.cal_dates
                                        .get(1)
                                        .or_else(|| view.cal_dates.first())
                                        .cloned()
                                        .unwrap_or_default()
                                } else {
                                    page.default_date.clone()
                                };
                            }
                        } else {
                            view.cal_items.extend(page.items.clone());
                        }
                        view.cal_next_offset = page.next_offset;
                        view.cal_session_id = page.session_id.clone();
                        view.cal_has_more = page.has_more;
                        // 混排页过滤后暂空但还有下一页：自动续拉（Tauri 同款）
                        let selected = view.cal_selected.clone();
                        let visible = view
                            .cal_items
                            .iter()
                            .filter(|i| selected.is_empty() || selected_date_of(i) == selected)
                            .count();
                        if visible == 0 && page.has_more && !more {
                            view.load_calendar(None, true, cx);
                        }
                    }
                    Err(e) => {
                        log::warn!("[New] 日历失败: {e}");
                        view.cal_failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
