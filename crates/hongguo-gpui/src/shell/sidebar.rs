use super::*;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, Role, SharedString, div, img};

use crate::ui_button::button as Button;

impl super::Shell {
    /// 侧栏折叠态落盘（`ui_prefs` 读-改-写，Tauri zustand persist 同语义）。
    fn persist_sidebar_collapsed(&self) {
        let mut prefs = crate::ui_prefs::load();
        prefs.sidebar_collapsed = self.sidebar_collapsed;
        crate::ui_prefs::save(prefs);
    }

    /// 侧栏（Tauri app-sidebar 对齐）：13 目的地，展开 160px 图标+文字、
    /// 折叠 56px 纯图标（tooltip 出标签）；追更带未读角标；贴底账户区
    /// （头像/昵称；已登录点进设置页）+ 折叠钮。
    ///
    /// 展开态比 Tauri 的 224px 窄：导航标签最长 4 个汉字（英文字段最长
    /// "Reservations"），224px 右侧会空掉近一百像素。160px 仍容得下最长
    /// 标签与追更角标，标签与昵称按 Tauri 同款截断（min-w-0 + truncate）。
    ///
    /// 导航行与账户行用组件库的**基础层**按钮（`gpui_kit::base::Button`）：
    /// 焦点句柄、Tab 可达、Enter/Space 激活、role/aria 与焦点环都由库负责，
    /// 视觉仍由本壳给（基础层不接管布局与配色）。
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
        // 贴底账户区（Tauri AccountButton 同位）：未登录开登录面板；
        // 已登录点进设置页（退出登录在设置页账户卡，Tauri 同款）
        let account_row = gpui_kit::base::Button::new("nav-account")
            .accessibility_label(account_label.clone())
            .flex()
            .items_center()
            .justify_start()
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
                s.child(
                    // 昵称按 Tauri 同款截断（`<span class="truncate">`）：
                    // 侧栏收窄后长昵称不能顶到右侧的「登录」标签上。
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(SharedString::from(account_label.clone())),
                )
                .children((!logged_in).then(|| {
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.primary)
                        .child(crate::i18n::t("nav.login"))
                }))
            });
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
            let label = dest.label();
            let row = gpui_kit::base::Button::new(ElementId::from(format!("nav-{}", label)))
                // 导航项是「链接到另一个目的地」，不是按钮（Tauri 侧同款 `<Link>`）。
                .role(Role::Link)
                .accessibility_label(label.clone())
                .aria_selected(active)
                .flex()
                .items_center()
                .justify_start()
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
                    // 折叠态：小角标压在图标右上（Tauri 同款）。用组件库的
                    // `Badge`：计数徽标的位置、圆角与「99+」封顶由库负责。
                    // `Badge` 未实现 `Styled`（不能链式加样式），固定尺寸的
                    // 外层 div 承担收缩策略。
                    div().flex_shrink_0().child(
                        gpui_kit::component::badge::Badge::new()
                            .count(if collapsed { badge.unwrap_or(0) } else { 0 })
                            .max(99)
                            .color(theme.danger)
                            .child(
                                gpui_kit::component::Icon::new(dest.icon())
                                    .small()
                                    .text_color(if active {
                                        theme.primary_foreground
                                    } else {
                                        theme.sidebar_foreground
                                    }),
                            ),
                    ),
                )
                .when(!collapsed, |s| {
                    // 标签按 Tauri 同款截断（`<span class="truncate">`）：
                    // 侧栏收窄后长标签只能缩，不能把行尾角标顶出去。
                    s.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(label.clone())),
                    )
                })
                // 展开态：数字胶囊贴行尾（Tauri 同款）。行尾徽标不是叠加在
                // 图标上的角标，用组件库的 `Tag`（小号 + 全圆角 = 药丸）。
                .children((!collapsed && badge.is_some()).then(|| {
                    gpui_kit::component::tag::Tag::danger()
                        .small()
                        .rounded_full()
                        .ml_auto()
                        .flex_shrink_0()
                        .child(SharedString::from(badge_text(badge.unwrap_or(0))))
                }));
            if collapsed {
                row.tooltip(move |window, cx| Tooltip::new(dest.label()).build(window, cx))
            } else {
                row
            }
        };
        v_flex()
            .w(gpui_kit::px(if collapsed { 56.0 } else { 160.0 }))
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
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_2()
                    .mt_auto()
                    .child(account_row),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .pt_2()
                    .child(if collapsed {
                        // 折叠态只剩一枚图标：组件 Button 的根是 shrink-to-fit
                        // 宽度，直接摆会贴内容区左边（实测偏左 7.5px）。撑满
                        // 整条内容区，图标才落在中线上——Tauri 侧同款
                        // `className="w-full"`（collapsed 时不加 justify-start，
                        // 由按钮自身居中）。
                        Button::new("sidebar-expand")
                            .icon(IconName::PanelLeft)
                            .ghost()
                            .small()
                            .w_full()
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
