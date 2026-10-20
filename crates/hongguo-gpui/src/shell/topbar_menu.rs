use super::*;

impl super::Shell {
    /// 顶栏中部 tab 槽的内容：当前目的地页面的分类 tab（Tauri 各页
    /// `TopBarTabsPortal` 同位）。没有 tab 的页面返回 None（槽留空）。
    pub(super) fn render_top_tabs(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        match self.dest {
            Dest::Home => self.feed.update(cx, |view, cx| view.render_top_tabs(cx)),
            Dest::Rank => self.rank.update(cx, |view, cx| view.render_top_tabs(cx)),
            Dest::New => self
                .new_drama
                .update(cx, |view, cx| view.render_top_tabs(cx)),
            _ => None,
        }
    }

    /// 顶栏外观菜单的落点：与设置页同一段实现（应用 + 落盘 + 通知观察者）。
    pub(super) fn set_theme_pref(
        &mut self,
        pref: crate::ui_prefs::ThemePref,
        cx: &mut Context<Self>,
    ) {
        self.settings_page
            .update(cx, |view, cx| view.set_theme(pref, cx));
    }

    /// 语言切换：更新进程内资源、持久化，并强制整窗重绘（子实体不各自
    /// 持有 locale，避免切换后只刷新壳、页面仍停在旧语言）。
    pub(super) fn set_locale(
        &mut self,
        locale: crate::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.locale == locale {
            return;
        }
        self.locale = locale;
        crate::i18n::set_locale(locale);
        let mut prefs = crate::ui_prefs::load();
        prefs.locale = locale;
        crate::ui_prefs::save(prefs);
        self.settings_page
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.feed
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.browse
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.history
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.collections
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.liked
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.reservations
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        self.storage
            .update(cx, |view, cx| view.refresh_locale(window, cx));
        if let Some(detail) = &self.detail {
            detail.update(cx, |view, cx| view.refresh_locale(window, cx));
        }
        cx.refresh_windows();
        cx.notify();
    }
}
