//! 排行榜列表数据：首拉/翻页、换 tab/子榜/筛选与封面懒加载。

use super::tabs::normalize_tabs;
use super::*;

impl super::RankView {
    /// 拉榜单：首屏或换 tab/子榜/筛选（`more` = 翻页）。
    ///
    /// 响应归属**发起请求时的键**（selected × sub × panel）：还在展示就整替
    /// /拼接展示态，不在展示就落进按键缓存，等切回去时恢复——切换期间
    /// 旧列表保持可见（keepPreviousData），不再清空 + 整页转圈。
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
        let list_key = (
            selected.clone(),
            sub.clone(),
            panel.clone().unwrap_or_default(),
        );
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
                let displaying = view.list_key() == list_key;
                match result {
                    Ok(page) => {
                        log::info!("[Rank] GPUI 壳 -> {} 条", page.items.len());
                        // 组装该键的完整列表：翻页在旧列表上拼接（展示中取
                        // 展示态，否则取按键缓存），首拉/换键整替。翻页响应
                        // 的键既不在展示也没缓存 = 用户已离开该榜单，丢弃。
                        let mut items = if more {
                            let base = if displaying {
                                Some(view.items.clone())
                            } else {
                                view.pages.get(&list_key).map(|cached| cached.items.clone())
                            };
                            match base {
                                Some(mut base) => {
                                    base.extend(page.items.clone());
                                    base
                                }
                                None => {
                                    cx.notify();
                                    return;
                                }
                            }
                        } else {
                            page.items.clone()
                        };
                        // recommend_info.rank 是页内位次（每页 1~10、可重复，
                        // 2026-10-17 解包实锤），连续名次按拼接序号重算；
                        // rank=0 是无名次榜（预约榜）保持 0
                        for (idx, it) in items.iter_mut().enumerate() {
                            if it.rank > 0 {
                                it.rank = (idx + 1) as u32;
                            }
                        }
                        // 版本戳全局记最新（官方客户端语义：跨 tab/子榜回传）
                        if !page.rank_version.is_empty() {
                            view.rank_version = page.rank_version.clone();
                        }
                        if !page.tabs.is_empty() {
                            view.raw_tabs = page.tabs.clone();
                        }
                        let page_state = RankListPage {
                            items,
                            next_offset: page.next_offset,
                            session_id: page.session_id.clone(),
                            has_more: page.has_more,
                        };
                        if displaying {
                            // 首页整替（含 needs_reload 补发、fallback 重拉）：
                            // 虚拟列表按整替对账（reset，滚动回顶）
                            if !more {
                                view.rows.replace();
                            }
                            view.items = page_state.items;
                            view.next_offset = page_state.next_offset;
                            view.session_id = page_state.session_id;
                            view.has_more = page_state.has_more;
                        } else {
                            view.pages.insert(list_key, page_state);
                        }
                        // 子榜失效（换 tab/形态切换）：落到第一个子榜重拉一次。
                        // 只在响应仍是展示键时跟进，人已切走就不追了。
                        if displaying && !more {
                            let tabs = normalize_tabs(&view.raw_tabs);
                            let current = tabs.iter().find(|t| t.id == view.selected);
                            let needs_fallback =
                                current.is_some_and(|t| !t.subs.iter().any(|s| s.id == view.sub));
                            if needs_fallback
                                && let Some(first) =
                                    current.and_then(|t| t.subs.first().map(|s| s.id.clone()))
                            {
                                view.sub = first;
                                view.panel.clear();
                                view.load_page(false, cx);
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("[Rank] GPUI 壳失败: {e}");
                        // 有旧列表在手（keepPreviousData 展示中）就保留展示，
                        // 静默失败等下次切换/重进重拉；真没数据才给失败态。
                        if displaying && view.items.is_empty() {
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

    /// 进入页面：有旧数据保留（刷新），否则首拉。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.items.is_empty() && !self.loading {
            self.load_page(false, cx);
        }
    }

    /// 换键（tab / 子榜 / 筛选）：旧键快照入缓存，新键有缓存立即恢复展示，
    /// 没有就保留旧列表展示（Tauri keepPreviousData 同语义）——只把翻页
    /// 游标作废，防止旧列表在新键名下续页。无论有无缓存都后台重拉一页
    /// （staleTime 0），滚动归零（Tauri 换 key 重挂载同语义）。
    fn switch_list(
        &mut self,
        selected: String,
        sub: String,
        panel: String,
        cx: &mut Context<Self>,
    ) {
        if self.selected == selected && self.sub == sub && self.panel == panel {
            return;
        }
        // 快照入缓存（**克隆**：旧列表要留在展示态给 keepPreviousData——
        // take 走的话缓存 miss 分支就没东西可展示了）
        self.pages.insert(
            self.list_key(),
            RankListPage {
                items: self.items.clone(),
                next_offset: self.next_offset,
                session_id: self.session_id.clone(),
                has_more: self.has_more,
            },
        );
        self.selected = selected;
        self.sub = sub;
        self.panel = panel;
        match self.pages.remove(&self.list_key()) {
            Some(cached) => {
                self.items = cached.items;
                self.next_offset = cached.next_offset;
                self.session_id = cached.session_id;
                self.has_more = cached.has_more;
            }
            None => {
                // 旧列表继续展示（keepPreviousData），只把翻页游标作废
                self.next_offset = 0;
                self.session_id.clear();
                self.has_more = false;
            }
        }
        self.failed = false;
        self.filter_open = false;
        // 虚拟列表按整替对账：reset 清测量并滚回顶（Tauri 换 key 重挂载同语义）
        self.rows.replace();
        if self.loading {
            // 旧键首拉在途：登记待重拉，响应落定后由 render 补发——load_page
            // 的单飞闸会吞掉这里的直接调用，漏了这步新键永远拉不到数据。
            self.needs_reload = true;
        } else {
            self.load_page(false, cx);
        }
        cx.notify();
    }

    pub(super) fn switch_to(&mut self, selected: String, sub: String, cx: &mut Context<Self>) {
        self.switch_list(selected, sub, String::new(), cx);
    }

    pub(super) fn pick_panel(&mut self, panel: String, cx: &mut Context<Self>) {
        let selected = self.selected.clone();
        let sub = self.sub.clone();
        self.switch_list(selected, sub, panel, cx);
    }
}
