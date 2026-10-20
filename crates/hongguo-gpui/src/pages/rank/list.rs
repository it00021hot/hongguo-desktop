//! 排行榜列表数据：首拉/翻页、换 tab/子榜/筛选与封面懒加载。

use super::tabs::normalize_tabs;
use super::*;

impl super::RankView {
    /// 拉榜单：首屏或换 tab/子榜/筛选（`more` = 翻页）。
    pub(super) fn load_page(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let selected = self.selected.clone();
        let sub = self.sub.clone();
        let panel = (!self.panel.is_empty()).then(|| self.panel.clone());
        let offset = if more { self.next_offset } else { 0 };
        let session = if more {
            self.session_id.clone()
        } else {
            String::new()
        };
        let rank_version = self.rank_version.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::rank::fetch_rank_ex(
                hongguo_api::rank::RankReq {
                    selected: &selected,
                    sub: &sub,
                    panel: panel.as_deref(),
                    background: None,
                    offset,
                    session_id: &session,
                    rank_version: &rank_version,
                },
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
                        log::info!("[Rank] GPUI 壳 -> {} 条", page.items.len());
                        if more {
                            view.items.extend(page.items.clone());
                        } else {
                            view.items = page.items.clone();
                        }
                        // recommend_info.rank 是页内位次（每页 1~10、可重复，
                        // 2026-10-17 解包实锤），连续名次按拼接序号重算；
                        // rank=0 是无名次榜（预约榜）保持 0
                        for (idx, it) in view.items.iter_mut().enumerate() {
                            if it.rank > 0 {
                                it.rank = (idx + 1) as u32;
                            }
                        }
                        view.next_offset = page.next_offset;
                        view.session_id = page.session_id.clone();
                        // 版本戳全局记最新（官方客户端语义：跨 tab/子榜回传）
                        if !page.rank_version.is_empty() {
                            view.rank_version = page.rank_version.clone();
                        }
                        view.has_more = page.has_more;
                        if !page.tabs.is_empty() {
                            view.raw_tabs = page.tabs.clone();
                        }
                        view.load_covers(&page.items, cx);
                        // 子榜失效（换 tab/形态切换）：落到第一个子榜重拉一次
                        let tabs = normalize_tabs(&view.raw_tabs);
                        let needs_fallback = tabs
                            .iter()
                            .find(|t| t.id == view.selected)
                            .map(|t| !t.subs.iter().any(|s| s.id == view.sub))
                            .unwrap_or(false);
                        if needs_fallback
                            && !more
                            && let Some(first) = tabs
                                .iter()
                                .find(|t| t.id == view.selected)
                                .and_then(|t| t.subs.first())
                                .map(|s| s.id.clone())
                        {
                            view.sub = first;
                            view.panel.clear();
                            view.load_page(false, cx);
                            return;
                        }
                    }
                    Err(e) => {
                        log::warn!("[Rank] GPUI 壳失败: {e}");
                        view.failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 进入页面：有旧数据保留（刷新），否则首拉。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.items.is_empty() && !self.loading {
            self.load_page(false, cx);
        }
    }

    pub(super) fn switch_to(&mut self, selected: String, sub: String, cx: &mut Context<Self>) {
        self.selected = selected;
        self.sub = sub;
        self.panel.clear();
        self.items.clear();
        self.next_offset = 0;
        self.session_id.clear();
        self.has_more = false;
        self.load_page(false, cx);
        cx.notify();
    }

    pub(super) fn pick_panel(&mut self, panel: String, cx: &mut Context<Self>) {
        self.panel = panel;
        self.items.clear();
        self.next_offset = 0;
        self.session_id.clear();
        self.has_more = false;
        self.filter_open = false;
        self.load_page(false, cx);
        cx.notify();
    }

    fn load_covers(&mut self, items: &[RankItem], cx: &mut Context<Self>) {
        for item in items {
            let key = item.cover.clone();
            if key.is_empty() || !self.covers_pending.insert(key.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(key.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&key);
                    if let Ok(bytes) = bytes
                        && let Ok(decoded) = image::load_from_memory(&bytes)
                    {
                        view.covers.insert(
                            key.clone(),
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
