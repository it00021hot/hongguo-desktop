use super::*;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, div, img};

impl super::Shell {
    /// 侧栏折叠态落盘（`ui_prefs` 读-改-写，Tauri zustand persist 同语义）。
    fn persist_sidebar_collapsed(&self) {
        let mut prefs = crate::ui_prefs::load();
        prefs.sidebar_collapsed = self.sidebar_collapsed;
        crate::ui_prefs::save(prefs);
    }

    /// 侧栏（Tauri app-sidebar 对齐）：13 目的地，展开 224px 图标+文字、
    /// 折叠 56px 纯图标（tooltip 出标签）；追更带未读角标；贴底账户区
    /// （头像/昵称；已登录点进设置页）+ 折叠钮。
    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let collapsed = self.sidebar_collapsed;
        // 头像懒加载守卫：账号在且 URL 变化才触发一次（幂等）
        self.ensure_account_avatar(cx);
        let theme = cx.theme();
        let account = self.state.settings().account;
        let logged_in = account.is_some();
        let unread = self.follow_unread;
        let avatar = self.account_avatar.clone();
        let account_label = account
            .as_ref()
            .map(|a| {
                if a.user_name.is_empty() {
                    crate::i18n::t("nav.accountFallback")
                } else {
                    a.user_name.clone()
                }
            })
            .unwrap_or_else(|| crate::i18n::t("nav.accountFallback"));
        let item = |dest: Dest| {
            let active = self.dest == dest;
            let badge = (dest == Dest::Follows && unread > 0).then_some(unread);
            let badge_text = |n: usize| {
                if n > 99 {
                    "99+".to_string()
                } else {
                    n.to_string()
                }
            };
            let row = div()
                .id(ElementId::from(format!("nav-{}", dest.label())))
                .flex()
                .items_center()
                .gap_3()
                .rounded_md()
                .px_3()
                .py_2()
                .text_sm()
                .cursor_pointer()
                .when(collapsed, |s| s.justify_center().px_0())
                .text_color(if active {
                    theme.primary_foreground
                } else {
                    theme.sidebar_foreground
                })
                .when(active, |s| {
                    s.bg(theme.primary)
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                })
                .hover(|s| {
                    s.bg(theme.sidebar_accent)
                        .text_color(theme.sidebar_accent_foreground)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.dest == dest {
                        return;
                    }
                    this.enter_dest(dest, cx);
                }))
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .child(
                            gpui_kit::component::Icon::new(dest.icon())
                                .small()
                                .text_color(if active {
                                    theme.primary_foreground
                                } else {
                                    theme.sidebar_foreground
                                }),
                        )
                        // 折叠态：小角标压在图标右上（Tauri 同款）
                        .children((collapsed && badge.is_some()).then(|| {
                            div()
                                .absolute()
                                .top(gpui_kit::px(-6.0))
                                .right(gpui_kit::px(-8.0))
                                .rounded_full()
                                .bg(theme.danger)
                                .px_1()
                                .text_size(gpui_kit::px(9.0))
                                .text_color(theme.danger_foreground)
                                .child(SharedString::from(badge_text(badge.unwrap_or(0))))
                        })),
                )
                .when(!collapsed, |s| s.child(dest.label()))
                // 展开态：数字胶囊贴行尾（Tauri 同款）
                .children((!collapsed && badge.is_some()).then(|| {
                    div()
                        .ml_auto()
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(theme.danger)
                        .px_1p5()
                        .py_0p5()
                        .text_size(gpui_kit::px(10.0))
                        .text_color(theme.danger_foreground)
                        .child(SharedString::from(badge_text(badge.unwrap_or(0))))
                }));
            if collapsed {
                row.tooltip(move |window, cx| Tooltip::new(dest.label()).build(window, cx))
            } else {
                row
            }
        };
        v_flex()
            .w(gpui_kit::px(if collapsed { 56.0 } else { 224.0 }))
            .h_full()
            .flex_shrink_0()
            .bg(theme.background)
            .border_r_1()
            .border_color(theme.border)
            .p_2()
            .gap_1()
            .id("shell-sidebar")
            .overflow_y_scroll()
            .children(Dest::ALL.map(item))
            // 贴底账户区（Tauri AccountButton 同位）：未登录开登录面板；
            // 已登录点进设置页（退出登录在设置页账户卡，Tauri 同款）
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_2()
                    .mt_auto()
                    .child(
                        div()
                            .id("nav-account")
                            .flex()
                            .items_center()
                            .gap_3()
                            .rounded_md()
                            .px_3()
                            .py(gpui_kit::px(7.0))
                            .text_sm()
                            .cursor_pointer()
                            .text_color(theme.muted_foreground)
                            .hover(|s| s.bg(theme.secondary_hover))
                            .when(collapsed, |s| s.justify_center().px_0())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if logged_in {
                                    this.enter_dest(Dest::Settings, cx);
                                } else {
                                    // 登录面板长在首页舞台：先切回首页再开
                                    this.dest = Dest::Home;
                                    this.feed.update(cx, |home, cx| {
                                        home.resume_if_idle(cx);
                                        home.toggle_login_panel(cx);
                                    });
                                    cx.notify();
                                }
                            }))
                            .child(match avatar {
                                Some(image) => img(ImageSource::Render(image))
                                    .size_5()
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element(),
                                None => gpui_kit::component::Icon::new(IconName::UserRound)
                                    .small()
                                    .text_color(theme.muted_foreground)
                                    .into_any_element(),
                            })
                            .when(!collapsed, |s| {
                                s.child(account_label.clone())
                                    .children((!logged_in).then(|| {
                                        div()
                                            .ml_auto()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(theme.primary)
                                            .child(crate::i18n::t("nav.login"))
                                    }))
                            }),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_2()
                    .child(if collapsed {
                        Button::new("sidebar-expand")
                            .icon(IconName::PanelLeft)
                            .ghost()
                            .small()
                            .tooltip(crate::i18n::t("nav.expand"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_collapsed = false;
                                this.persist_sidebar_collapsed();
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        Button::new("sidebar-collapse")
                            .icon(IconName::PanelLeftClose)
                            .ghost()
                            .small()
                            .label(crate::i18n::t("nav.collapse"))
                            .tooltip(crate::i18n::t("nav.collapse"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_collapsed = true;
                                this.persist_sidebar_collapsed();
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }
}
