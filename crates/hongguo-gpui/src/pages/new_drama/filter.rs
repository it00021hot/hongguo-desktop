use super::*;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;

impl NewDramaView {
    /// 切频道：旧频道快照入缓存，看过的频道立即恢复展示；没缓存的频道
    /// 保留旧列表展示（Tauri keepPreviousData 同语义，翻页游标作废），
    /// 无论有无缓存都后台重拉一页，滚动归零（Tauri 换 key 重挂载同语义）。
    fn switch_gender(&mut self, gender: i64, cx: &mut Context<Self>) {
        if self.gender == gender {
            return;
        }
        // 快照入缓存（**克隆**：旧列表要留在展示态给 keepPreviousData）
        self.gender_pages.insert(
            self.gender,
            GenderPage {
                items: self.items.clone(),
                next_offset: self.next_offset,
                session_id: self.session_id.clone(),
                has_more: self.has_more,
            },
        );
        self.gender = gender;
        match self.gender_pages.remove(&gender) {
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
        // 虚拟列表按整替对账：reset 清测量并滚回顶（Tauri 换 key 重挂载同语义）
        self.rec_rows.replace();
        if self.loading {
            // 旧频道首拉在途：登记待重拉，响应落定后由 render 补发。
            self.recommend_needs_reload = true;
        } else {
            self.load_recommend(false, cx);
        }
        cx.notify();
    }

    // —— 渲染 ——

    /// 视图 tab（Tauri `TopBarTabs` portal 同位：壳在顶栏渲染它，
    /// 页面主体的那行只剩频道胶囊——见 [`Self::render_gender_row`]）。
    pub(crate) fn render_top_tabs(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let view = self.view;
        Some(
            h_flex()
                .id("top-new-tabs")
                .items_center()
                .gap_0p5()
                .children(
                    [
                        (
                            ViewTab::Recommend,
                            crate::i18n::t("newDrama.tabs.recommend"),
                        ),
                        (ViewTab::Calendar, crate::i18n::t("newDrama.tabs.calendar")),
                    ]
                    .map(|(tab, label)| {
                        let active = view == tab;
                        let button = crate::pill_tab::pill_tab(
                            format!("new-tab-{label}"),
                            label,
                            active,
                            cx,
                        );
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            if this.view == tab {
                                return;
                            }
                            this.view = tab;
                            if tab == ViewTab::Calendar && !this.cal_loaded {
                                this.load_calendar(None, false, cx);
                            }
                            cx.notify();
                        }))
                    }),
                )
                .into_any_element(),
        )
    }

    /// 频道胶囊行（Tauri 新剧页同款：只属于新剧推荐视图、贴左排）。
    ///
    /// Tauri 代码里写的是 `justify-end`，但 ToggleGroup 基础类是 inline-flex
    /// （宽度收缩到内容），`justify-end` 实际不生效——真机渲染是贴左。这里
    /// 以真机渲染为口径，不照抄失效的类名。
    pub(super) fn render_gender_row(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        if self.view != ViewTab::Recommend {
            return None;
        }
        let theme = cx.theme();
        let gender = self.gender;
        Some(
            h_flex()
                .id("new-gender-row")
                .w_full()
                .items_center()
                .gap_2()
                .children(
                    [
                        (2, crate::i18n::t("newDrama.gender.all")),
                        (1, crate::i18n::t("newDrama.gender.male")),
                        (0, crate::i18n::t("newDrama.gender.female")),
                    ]
                    .map(|(value, label)| {
                        let active = gender == value;
                        let button = crate::pill_tab::pill_tab(
                            format!("new-gender-{label}"),
                            label,
                            active,
                            cx,
                        );
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.switch_gender(value, cx);
                        }))
                    }),
                )
                .text_color(theme.foreground)
                .into_any_element(),
        )
    }
}
