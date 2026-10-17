//! 登录态与壳内 SMS 登录（发码/提交/MFA 后台轮询）、登录面板渲染与
//! 评论/回复头像的拉取与渲染。

use std::sync::Arc;
#[cfg(feature = "screenshot")]
use std::time::Duration;
#[cfg(feature = "screenshot")]
use std::time::Instant;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{SharedString, div};

#[cfg(feature = "screenshot")]
use super::controls::CONTROLS_HOLD_SECS;
use crate::bridge;
use hongguo_service::login_service::flow::LoginResult;

impl super::HomeView {
    /// 登录态（settings 账号卡片的 user_id 非空）。
    pub(super) fn logged_in(&self) -> bool {
        !self.my_user_id().is_empty()
    }

    /// 当前登录用户 id（未登录空串）：评论/回复「删除」按钮的 isMine 判据
    ///（Tauri myUserId 同口径）。
    pub(super) fn my_user_id(&self) -> String {
        self.state
            .settings()
            .account
            .map(|a| a.user_id)
            .unwrap_or_default()
    }

    /// 打开/关闭登录面板（Shell 侧栏入口）。
    pub(crate) fn toggle_login_panel(&mut self, cx: &mut Context<Self>) {
        self.login_panel_open = !self.login_panel_open;
        self.login_message = None;
        cx.notify();
    }

    /// 退出登录（settings.account 清空落库；Tauri login_logout 同口径）。
    pub(crate) fn login_logout(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = hongguo_service::login_service::flow::logout(&self.state) {
            log::warn!("[Login] 退出失败: {e}");
            return;
        }
        // 登出即离开播放面（Tauri 登出入口在设置页、播放器早已卸载；本壳
        // 侧栏直接登出，按卸载语义先停播落进度）
        self.release_for_leave(cx);
        self.mfa_retry = None;
        self.login_message = Some("已退出登录".to_string());
        log::info!("[Login] 已退出登录");
        cx.emit(super::HomeEvent::AccountChanged);
        cx.notify();
    }

    /// 发送短信验证码（编排在 service 的 `login_service::flow`，
    /// 与 Tauri login_send_code 同一份；csrf 凭据由 service 暂存）。
    fn login_send_code(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.login_mobile else {
            return;
        };
        let mobile = input.read(cx).value().trim().to_string();
        if mobile.len() != 11 || !mobile.starts_with('1') || self.login_busy {
            self.login_message = Some("手机号格式不正确".into());
            cx.notify();
            return;
        }
        self.login_busy = true;
        self.login_message = None;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            hongguo_service::login_service::flow::send_code(&state, &mobile).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.login_busy = false;
                match result {
                    Ok(outcome) => {
                        // csrf 凭据已由 service 暂存（提交时消费），壳不再经手
                        log::info!("[Login] 发码成功: {}", outcome.message);
                        view.login_message = Some(format!("{}（注意查收短信）", outcome.message));
                    }
                    Err(e) => {
                        log::warn!("[Login] 发码失败: {e}");
                        view.login_message = Some(format!("发码失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 短信验证码登录：编排全在 service 的 `login_service::flow`（落库 +
    /// MFA 后台轮询 + 资料刷新与 Tauri 同一份）。MFA 转后台后，状态经
    /// `login-mfa-state` 事件回到 [`HomeView::on_login_mfa_event`]。
    fn login_submit(&mut self, cx: &mut Context<Self>) {
        let Some(mobile_input) = &self.login_mobile else {
            return;
        };
        let Some(code_input) = &self.login_code else {
            return;
        };
        let mobile = mobile_input.read(cx).value().trim().to_string();
        let code = code_input.read(cx).value().trim().to_string();
        if mobile.len() != 11 || !mobile.starts_with('1') {
            self.login_message = Some("手机号格式不正确".into());
            cx.notify();
            return;
        }
        if code.is_empty() || self.login_busy {
            return;
        }
        self.login_busy = true;
        self.login_message = None;
        let state = Arc::clone(&self.state);
        let mfa = self.mfa_retry.clone();
        let rx = bridge::spawn(async move {
            let mfa = mfa.as_ref().map(|(t, k)| (t.as_str(), k.as_str()));
            hongguo_service::login_service::flow::submit(&state, &mobile, &code, mfa).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match result {
                Err(e) => {
                    view.login_busy = false;
                    log::warn!("[Login] 登录失败: {e}");
                    view.login_message = Some(format!("登录失败：{e}"));
                    cx.notify();
                }
                Ok(LoginResult::Success { user }) => {
                    view.login_busy = false;
                    view.mfa_retry = None;
                    log::info!("[Login] 登录成功: {} ({})", user.name, user.user_id);
                    // 账号落库与资料刷新（真昵称/高清头像）由 service 的
                    // complete_login 后台完成——壳不再自行跟进
                    view.login_message = Some(format!("登录成功：{}", user.name));
                    cx.emit(super::HomeEvent::AccountChanged);
                    cx.notify();
                }
                Ok(LoginResult::Mfa {
                    retry_tag,
                    sms_code_key,
                    tips,
                    ..
                }) => {
                    view.login_busy = false;
                    view.mfa_retry = Some((retry_tag, sms_code_key));
                    log::info!("[Login] 需要短信上行验证，转后台自动轮询");
                    // 后续状态（等待/成功/失败）经 login-mfa-state 事件回来
                    view.login_message = Some(tips);
                    cx.notify();
                }
                Ok(LoginResult::MfaWaiting) => {
                    // 单次校验路径的结果；提交路径理论上不会出现
                    view.login_busy = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 登录 MFA 状态事件（service 后台轮询的出口，与 Tauri 壳同一批事件）：
    /// 更新面板文案与重试上下文。轮询本身在 service 里跑（进程级任务运行时），
    /// 壳只做 UI 适配——之前这里有一份与 service 逐行对应的轮询复刻。
    pub(super) fn on_login_mfa_event(
        &mut self,
        ev: hongguo_service::login_service::events::LoginMfaStateEvent,
        cx: &mut Context<Self>,
    ) {
        use hongguo_service::login_service::events::LoginMfaStateEvent;
        match ev {
            LoginMfaStateEvent::Success { name } => {
                self.login_busy = false;
                self.mfa_retry = None;
                self.login_message = Some(format!("登录成功：{name}"));
                cx.emit(super::HomeEvent::AccountChanged);
            }
            LoginMfaStateEvent::Failed {
                message_kind,
                message,
            } => {
                self.login_busy = false;
                self.mfa_retry = None;
                self.login_message = Some(if message_kind == "settings.loginMfaTimeout" {
                    "5 分钟未确认，登录已超时退出".into()
                } else {
                    format!("登录失败：{message}")
                });
            }
            LoginMfaStateEvent::Waiting { server_tip } => {
                self.login_busy = false;
                self.login_message = Some(server_tip);
            }
        }
        cx.notify();
    }

    /// 拉评论/回复头像（avatar URL → RGBA）。同一阶梯（嗅探透传 jpeg，
    /// 失败退回首字圆形占位）；key 就是 URL，列表里同人多条只拉一次。
    /// `on_miss` 场景不重试：头像属装饰信息，失败静默（与封面同口径）。
    pub(super) fn load_avatar(&mut self, url: String, cx: &mut Context<Self>) {
        if url.is_empty()
            || self.avatars.contains_key(&url)
            || !self.avatars_pending.insert(url.clone())
        {
            return;
        }
        let rx = bridge::spawn({
            let remote = url.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    hongguo_protocol::cover::acquire_renderable(&remote).map(|(bytes, _)| bytes)
                })
                .await
                .expect("阻塞池 join")
            }
        });
        cx.spawn(async move |this, cx| {
            let bytes: Vec<u8> = match rx.await.expect("tokio 桥不会丢弃发送端") {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("[Home] 头像加载失败: {e}");
                    return;
                }
            };
            let _ = this.update(cx, |view, cx| match image::load_from_memory(&bytes) {
                Ok(decoded) => {
                    view.avatars.insert(
                        url,
                        Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                    );
                    cx.notify();
                }
                Err(e) => log::warn!("[Home] 头像解码失败: {e}"),
            });
        })
        .detach();
    }

    /// 登录面板（居中卡片）：已登录显示账号+退出；未登录显示手机号/验证码
    /// 表单 + 登录按钮。点遮罩收起。
    pub(super) fn render_login_panel(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let account = self.state.settings().account;
        // 按钮门槛与 Tauri 登录弹窗同款：手机号 11 位、验证码 ≥4 位
        let mobile = self
            .login_mobile
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        let code = self
            .login_code
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        let mobile_ok = mobile.len() == 11 && mobile.starts_with('1');
        let code_ok = code.len() >= 4;
        let theme = cx.theme();
        v_flex()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::rgba(0x00000099))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.login_panel_open = false;
                    cx.notify();
                }),
            )
            .child({
                let mobile_input = self.login_mobile.clone();
                let code_input = self.login_code.clone();
                v_flex()
                    // Tauri 登录弹窗 512×383（DialogContent max-w-sm + p-6）
                    .w(gpui_kit::px(512.0))
                    .p_6()
                    .gap_4()
                    .rounded(gpui_kit::px(10.0))
                    .bg(gpui_kit::rgb(0x1f1f1f))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    // 头部：标题 + 说明 + 关闭（Tauri DialogHeader 同构）
                    .child(
                        h_flex()
                            .items_start()
                            .justify_between()
                            .gap_2()
                            .child(
                                v_flex()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .text_size(gpui_kit::px(18.0))
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(theme.foreground)
                                            .child(match &account {
                                                Some(a) => SharedString::from(format!(
                                                    "账号：{}",
                                                    a.user_name
                                                )),
                                                None => "登录红果账号".into(),
                                            }),
                                    )
                                    .children(account.is_none().then(|| {
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child("登录后可预约新剧、同步观看记录")
                                    })),
                            )
                            .child(
                                Button::new("login-close")
                                    .icon(gpui_kit::assets::IconName::X)
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.login_panel_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when_some(account.clone(), |s, a| {
                        s.child(
                            div()
                                .text_xs()
                                .text_color(gpui_kit::rgba(0xffffff99))
                                .child(SharedString::from(format!("UID {}", a.user_id))),
                        )
                        .child(
                            Button::new("login-logout")
                                .label("退出登录")
                                .danger()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.login_logout(cx);
                                    this.login_panel_open = false;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(account.is_none(), |s| {
                        s.child(
                            // 手机号：字段标签 + 输入（Tauri FieldLabel 同款）
                            v_flex()
                                .gap_1p5()
                                .child(div().text_sm().text_color(theme.foreground).child("手机号"))
                                .children(
                                    mobile_input
                                        .as_ref()
                                        .map(|input| Input::new(input).h(gpui_kit::px(36.0))),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1p5()
                                .child(div().text_sm().text_color(theme.foreground).child("验证码"))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .children(code_input.as_ref().map(|input| {
                                            Input::new(input).flex_1().h(gpui_kit::px(36.0))
                                        }))
                                        .child(
                                            Button::new("login-send-code")
                                                .icon(gpui_kit::assets::IconName::Send)
                                                .label(if self.login_busy {
                                                    "发送中…"
                                                } else {
                                                    "发送验证码"
                                                })
                                                .outline()
                                                .small()
                                                .disabled(self.login_busy || !mobile_ok)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.login_send_code(cx);
                                                })),
                                        ),
                                ),
                        )
                        .child(
                            Button::new("login-submit")
                                .label(if self.mfa_retry.is_some() {
                                    "重试登录"
                                } else {
                                    "登录"
                                })
                                .primary()
                                .w_full()
                                .disabled(self.login_busy || !mobile_ok || !code_ok)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.login_submit(cx);
                                })),
                        )
                        .child(
                            // Tauri loginRiskNote 原文
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("账号与设备永久绑定，换设备登录可能触发风控"),
                        )
                    })
                    .children(self.login_message.clone().map(|msg| {
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0x93c5fdff))
                            .child(SharedString::from(msg))
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    /// 头像元素：有图用图（圆形裁切），无图/未加载退回昵称首字圆形占位。
    /// 外层固定尺寸框是必须的——img 直接 size_full 在无约束 flex 里会撑爆。
    pub(super) fn render_avatar(&self, url: &str, name: &str, size: f32) -> gpui_kit::AnyElement {
        let frame = div()
            .flex_shrink_0()
            .rounded_full()
            .overflow_hidden()
            .size(gpui_kit::px(size));
        match self.avatars.get(url) {
            Some(img) => frame
                .child(
                    gpui_kit::img(gpui_kit::ImageSource::Render(Arc::clone(img)))
                        .size_full()
                        .object_fit(gpui_kit::ObjectFit::Cover),
                )
                .into_any_element(),
            None => frame
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::rgba(0x404040ff))
                .text_xs()
                .text_color(gpui_kit::rgb(0xffffff))
                .child({
                    let ch = name.chars().next().unwrap_or('游');
                    SharedString::from(ch.to_string())
                })
                .into_any_element(),
        }
    }

    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PANEL=login）：打开
    /// 登录面板（无人值守抓登录 UI）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_login(&mut self, cx: &mut Context<Self>) {
        self.login_panel_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        cx.notify();
    }
}
