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
                        if more {
                            view.items.extend(page.items.clone());
                        } else {
                            view.items = page.items.clone();
                        }
                        view.next_offset = page.next_offset;
                        view.session_id = page.session_id.clone();
                        view.has_more = page.has_more;
                        let urls: Vec<String> =
                            page.items.iter().map(|i| i.cover.clone()).collect();
                        view.load_covers(&urls, cx);
                    }
                    Err(e) => {
                        log::warn!("[New] 推荐流失败: {e}");
                        view.failed = true;
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
                        let urls: Vec<String> =
                            page.items.iter().map(|i| i.cover.clone()).collect();
                        view.load_covers(&urls, cx);
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

    fn load_covers(&mut self, urls: &[String], cx: &mut Context<Self>) {
        for url in urls {
            let url = url.clone();
            if url.is_empty() || !self.covers_pending.insert(url.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(url.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&url);
                    if let Ok(bytes) = bytes
                        && let Ok(decoded) = image::load_from_memory(&bytes)
                    {
                        view.covers.insert(
                            url.clone(),
                            Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                        );
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }
}
