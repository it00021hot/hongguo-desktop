//! 预约页列表数据：首页/翻页加载、搜索补齐、封面与预览种子。

use super::*;

impl ReservationView {
    /// 拉两个 tab 的预约列表首页（角标计数两条都要，Tauri 双 query 同款）。
    /// 单页即回（Tauri 同构懒翻页）：此前一次性串行拉全，预约上百部时
    /// 首屏要等几十个请求——issue #11 的「已上线 5–8 秒才出来」。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        let account = self.state.settings().account.clone();
        if account.is_none() {
            // 未登录：不拉（接口匿名只给空表），页面走登录入口
            self.online = TabData::default();
            self.offline = TabData::default();
            cx.notify();
            return;
        }
        if self.online.loading || self.offline.loading {
            return;
        }
        for online in [true, false] {
            let tab = if online {
                &mut self.online
            } else {
                &mut self.offline
            };
            if tab.items.is_none() {
                tab.loading = true;
            }
            tab.failed = None;
        }
        cx.notify();
        for online in [true, false] {
            self.fetch_tab(online, 0, String::new(), false, cx);
        }
    }

    /// 当前 tab 加载更多（进行中/没有下页时忽略；搜索态的后台补齐同入口）。
    pub(crate) fn load_more(&mut self, cx: &mut Context<Self>) {
        {
            let tab = self.current_mut();
            if !tab.has_more || tab.loading_more || tab.loading {
                return;
            }
            if tab.items.is_some() {
                tab.loading_more = true;
            }
        }
        let (offset, cursor) = {
            let tab = self.current();
            (tab.next_offset, tab.cursor.clone())
        };
        cx.notify();
        self.fetch_tab(self.online_tab, offset, cursor, true, cx);
    }

    /// 搜索词变化：非空时对当前 tab 后台补齐剩余页——搜索要对全量条目
    /// 过滤（hgplayer「正在加载全部记录…」同款语义），普通浏览一页懒翻。
    pub(crate) fn on_search_changed(&mut self, cx: &mut Context<Self>) {
        let q = self.search.read(cx).value().trim().to_lowercase();
        if !q.is_empty() {
            self.load_more(cx);
        }
    }

    /// 拉一个 tab 的一页（`extend=false` 首页替换并取封面 / `true` 续拉追加）。
    fn fetch_tab(
        &mut self,
        online: bool,
        offset: i64,
        session_id: String,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::fetch_reservations_page(
                online,
                offset,
                Some(session_id.as_str()).filter(|s| !s.is_empty()),
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                let Ok(page) = result else {
                    let e = result.unwrap_err();
                    log::warn!("[Reservations] GPUI 壳失败 online={online}: {e}");
                    let tab = if online {
                        &mut view.online
                    } else {
                        &mut view.offline
                    };
                    tab.loading = false;
                    tab.loading_more = false;
                    if tab.items.is_none() {
                        tab.failed = Some(crate::i18n::app_error_text(&e));
                    }
                    cx.notify();
                    return;
                };
                log::info!(
                    "[Reservations] GPUI 壳 online={online} -> {} 条",
                    page.items.len()
                );
                // 首页先取封面再动 tab 借用；解构终结 page，游标三件套随后写回
                let hongguo_api::calendar::CalendarPage {
                    items,
                    session_id,
                    next_offset,
                    has_more,
                    online_total,
                    offline_total,
                    ..
                } = page;
                let tab = if online {
                    &mut view.online
                } else {
                    &mut view.offline
                };
                tab.loading = false;
                tab.loading_more = false;
                if extend {
                    tab.extend_page(items);
                } else {
                    tab.total = if online { online_total } else { offline_total };
                    tab.items = Some(items);
                }
                tab.cursor = session_id;
                tab.next_offset = next_offset;
                tab.has_more = has_more && next_offset > 0;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 预览种子（screenshot feature）：灌假预约列表，跳过网络。
    /// `reservation-offline` = 待上线 tab；`reservation-empty` = 空态；
    /// `reservation-manage` = 管理模式（前两条已选）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let empty = scene == "reservation-empty";
        let online = !scene.contains("offline");
        self.online_tab = online;
        let mut online_items = if empty || !online {
            Vec::new()
        } else {
            crate::preview::fixtures::reservation_items(true)
        };
        let mut offline_items = if empty || online {
            Vec::new()
        } else {
            crate::preview::fixtures::reservation_items(false)
        };
        for (i, item) in offline_items.iter_mut().enumerate() {
            let url = format!("preview://reservation-offline/{i}");
            item.cover = url.clone();
            crate::preview::fixtures::put_preview_image(
                url,
                crate::preview::fixtures::cover(6 + i),
            );
        }
        for (i, item) in online_items.iter_mut().enumerate() {
            let url = format!("preview://reservation-online/{i}");
            item.cover = url.clone();
            crate::preview::fixtures::put_preview_image(url, crate::preview::fixtures::cover(i));
        }
        self.online = TabData {
            total: online_items.len() as i64,
            items: Some(online_items),
            loading: false,
            failed: None,
            ..Default::default()
        };
        self.offline = TabData {
            total: offline_items.len() as i64,
            items: Some(offline_items),
            loading: false,
            failed: None,
            ..Default::default()
        };
        if scene == "reservation-manage" {
            self.managing = true;
            self.selected = self.seed_selection().into_iter().collect();
        }
        self.preview_logged_in = true;
        cx.notify();
    }

    #[cfg(feature = "screenshot")]
    fn seed_selection(&self) -> Vec<String> {
        self.online
            .items
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .take(2)
            .map(|i| i.series_id.clone())
            .collect()
    }
}
