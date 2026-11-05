use super::*;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, Role, SharedString, div, img};

impl super::Shell {
    /// 侧栏折叠态落盘（`ui_prefs` 读-改-写，Tauri zustand persist 同语义）。
    pub(super) fn persist_sidebar_collapsed(&self) {
        let mut prefs = crate::ui_prefs::load();
        prefs.sidebar_collapsed = self.sidebar_collapsed;
        crate::ui_prefs::save(prefs);
    }

    /// 侧栏：13 目的地，展开 160px 图标+文字、折叠 56px 纯图标（tooltip 出
    /// 标签）；追更带未读角标；贴底账户区（未登录开登录面板，已登录进设置页）。
    /// 折叠入口在顶栏（红绿灯同行）。导航行与账户行用组件库基础层按钮。
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
        // 贴底账户区：未登录开登录面板；已登录点进设置页（退出登录在设置页账户卡）
        let account_row = gpui_kit::base::Button::new("nav-account")
            .accessibility_label(account_label.clone())
            .flex()
            .items_center()
            .justify_start()
            .gap_3()
            .rounded_md()
            .px_3()
            .py(gpui_kit::px(7.0))
            // rem=14（Codex 界面字号）时 text_base=14px
            .text_base()
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
                    // Tauri 侧栏 `size-4`（16px），勿用 `.small()`（14px）
                    .with_size(gpui_kit::px(16.0))
                    .text_color(theme.muted_foreground)
                    .into_any_element(),
            })
            .when(!collapsed, |s| {
                // 未登录也只显示「我的」：整行可点开登录，不再旁挂「登录」二字。
                s.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(SharedString::from(account_label.clone())),
                )
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
                // rem=14 时 text_base=14px，对齐 Codex 界面字号
                .text_base()
                .cursor_pointer()
                .when(collapsed, |s| s.justify_center().px_0())
                .text_color(theme.sidebar_foreground)
                // Codex 选中：淡底+字重，不反色（非 primary 实心块）
                .when(active, |s| {
                    s.bg(theme.sidebar_primary)
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
                                    // Tauri `<Icon className="size-4" />`
                                    .with_size(gpui_kit::px(16.0))
                                    .text_color(theme.sidebar_foreground),
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
        // Codex：侧栏通顶——顶部 44px 留给红绿灯（mac）/作拖拽带，导航在下方。
        let mac = cfg!(target_os = "macos");
        // 折叠宽：mac 至少 76 才能罩住 trafficLight x=12 起的三灯；其它平台 56。
        let rail_w = if collapsed {
            if mac { 76.0 } else { 56.0 }
        } else {
            220.0
        };
        // 首页视频贴右边：亮色分隔线会看成白边，这条边不画。
        let home = self.dest == Dest::Home;
        v_flex()
            .w(gpui_kit::px(rail_w))
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar)
            .when(!home, |s| s.border_r_1().border_color(theme.sidebar_border))
            .id("shell-sidebar")
            .child(
                // 顶带：与内容列顶栏同高，红绿灯落在此；空白可拖窗。
                // 不加底边——Codex 侧栏从上到下是一整块，横线只画在内容列顶栏。
                div()
                    .h(gpui_kit::px(44.0))
                    .w_full()
                    .flex_shrink_0(),
            )
            .child(
                v_flex()
                    .id("shell-sidebar-nav")
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .gap_0p5()
                    .overflow_y_scroll()
                    .children(Dest::ALL.map(item))
                    .child(
                        div()
                            .border_t_1()
                            .border_color(theme.sidebar_border)
                            .pt_2()
                            .mt_auto()
                            .child(account_row),
                    ),
            )
            .into_any_element()
    }
}
