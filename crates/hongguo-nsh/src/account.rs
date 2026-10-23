//! 账号域（壳级）：全局登录态 + 短信登录面板。
//!
//! 编排全在 service 的 `login_service::flow`（发码 / 提交 / MFA 后台轮询 /
//! 落库），壳只做 UI 适配——GPUI 壳 `home/auth.rs` 与 Tauri 壳
//! `login-dialog` 的同一份下游。MFA 后台轮询的状态经事件总线
//! （`pump::ShellEvents::login_mfa`）回到面板；登录态是壳级信号，
//! 互动栏 / 评论 / 追更的登录墙都读它。

use std::sync::LazyLock;

use nsh_core::ui::prelude::*;

use hongguo_service::login_service::flow::{self, LoginResult};

use crate::i18n::t;
use crate::pump;
use crate::tokens::scrim;

/// 登录态（settings 账号档案的镜像；登录/退出时翻沿）。
static LOGGED_IN: LazyLock<Signal<bool>> = LazyLock::new(|| {
    signal_with(
        crate::state::state().settings().account.is_some(),
        SignalEquals::Never,
    )
});

/// 登录面板开合。
static OPEN: LazyLock<Signal<bool>> = LazyLock::new(|| signal(false));

/// 组件体里触碰一次：信号节点跟着组件树建起来（首个使用点可能在更深
/// 的组件里，那里不再是组件体）。
pub fn init() {
    LazyLock::force(&LOGGED_IN);
    LazyLock::force(&OPEN);
}

/// 登录态（视图里读它切分支）。
pub fn logged_in() -> Signal<bool> {
    *LOGGED_IN
}

/// 翻登录态（登录成功 / 退出；幂等）。
pub fn set_logged_in(on: bool) {
    if LOGGED_IN.with(|v| *v) != on {
        log::info!("[Login] 登录态 → {on}");
        LOGGED_IN.set(on);
    }
}

/// 打开登录面板（设置页账号卡 / 追更页登录墙共用入口）。
pub fn open_login() {
    OPEN.set(true);
}

/// 当前账号（未登录 None）。
pub fn current() -> Option<hongguo_common::model::AccountState> {
    crate::state::state().settings().account
}

/// 手机号形态（11 位、1 开头；服务端会再校验）。
fn valid_mobile(mobile: &str) -> bool {
    mobile.len() == 11 && mobile.starts_with('1')
}

/// 面板卡宽（Tauri 登录弹窗 max-w-sm 一档）。
const PANEL_W: f32 = 440.0;

/// 登录面板（居中卡片；点遮罩收起）。挂在 content() 里，开合由 [`OPEN`]
/// 驱动。**不是 `#[component]`**：组件包装节点会让 absolute 遮罩回落到
/// 静态位置（实测整树沉底且被裁掉），普通函数 + `If` 与 download 面板
/// 同构，定位才是可靠的。
pub fn login_overlay() -> impl IntoElement {
    let mobile = signal(String::new());
    let code = signal(String::new());
    let busy = signal(false);
    let message = signal(String::new());
    // MFA 重试上下文（retry_tag, sms_code_key）；`Some` = 等短信上行。
    let mfa_retry: Stored<Option<(String, String)>> = stored(None);
    let confirm_logout = signal(false);
    let events = pump::events();

    // MFA 后台轮询的出口（service → pump）：成功/失败收尾，等待中刷提示。
    // 登录态翻沿不在面板里做——面板关着时轮询也能完成，pump 是唯一出口。
    {
        let (busy, mfa_retry, message) = (busy, mfa_retry, message);
        effect(move || {
            let Some(ui) = events.get().login_mfa.clone() else {
                return;
            };
            match ui {
                pump::MfaUi::Success { name } => {
                    busy.set(false);
                    mfa_retry.replace(None);
                    message.set(t("settings.loginSuccess").replace("{name}", &name));
                }
                pump::MfaUi::Failed {
                    timeout,
                    message: text,
                } => {
                    busy.set(false);
                    mfa_retry.replace(None);
                    message.set(if timeout {
                        t("settings.loginMfaTimeout")
                    } else {
                        t("settings.loginFailed").replace("{message}", &text)
                    });
                }
                pump::MfaUi::Waiting { tip } => {
                    busy.set(false);
                    message.set(tip);
                }
            }
        });
    }

    If::new(
        move || OPEN.get(),
        move || panel_body(mobile, code, busy, message, mfa_retry, confirm_logout),
    )
}

fn panel_body(
    mobile: Signal<String>,
    code: Signal<String>,
    busy: Signal<bool>,
    message: Signal<String>,
    mfa_retry: Stored<Option<(String, String)>>,
    confirm_logout: Signal<bool>,
) -> impl IntoElement {
    let msg_row = message;
    view()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .flex_col()
        .items_center()
        .justify_center()
        .bg(scrim())
        // 点遮罩收起（面板内容自己吞点击，落不到这层）。
        .on_press(move |_: &()| {
            confirm_logout.set(false);
            OPEN.set(false);
        })
        .child(
            view()
                .w(px(PANEL_W))
                .flex_col()
                .gap(px(14.0))
                .p(px(24.0))
                .rounded(px(12.0))
                .bg(theme().group_box)
                .border_1()
                .border_color(theme().border)
                .on_press(move |_: &()| {})
                // 头部：标题 + 说明 + 关闭。
                .child(
                    view()
                        .w_full()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            view().flex_col().gap(px(4.0)).child(
                                text(move || match current() {
                                    Some(account) if !account.user_name.is_empty() => {
                                        t("settings.accountId").replace("{id}", &account.user_name)
                                    }
                                    _ => t("settings.loginTitle"),
                                })
                                .text_size(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme().foreground),
                            ),
                        )
                        .child(
                            text("✕")
                                .text_size(px(14.0))
                                .text_color(theme().muted_foreground)
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded(px(6.0))
                                .key("login-close")
                                .on_press(move |_: &()| OPEN.set(false)),
                        ),
                )
                // 已登录：账号信息 + 退出（两段式确认：点一次变「确定退出？」）。
                .child(If::new(
                    move || logged_in().get(),
                    move || {
                        let Some(account) = current() else {
                            return view().into_element();
                        };
                        let uid = account.user_id.clone();
                        let masked = mask_mobile(&account.mobile);
                        let has_mobile = !masked.is_empty();
                        view()
                            .w_full()
                            .flex_col()
                            .gap(px(10.0))
                            .child(
                                view()
                                    .w_full()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(
                                        text(move || uid.clone())
                                            .text_size(px(13.0))
                                            .text_color(theme().muted_foreground),
                                    )
                                    .child(If::new(
                                        move || has_mobile,
                                        move || {
                                            let masked = masked.clone();
                                            text(move || masked.clone())
                                                .text_size(px(13.0))
                                                .text_color(theme().muted_foreground)
                                        },
                                    )),
                            )
                            .child(
                                // text 上的 items/justify 不居中内容：固定高
                                // 按钮一律 view 包 text（壳内 mini-toggle 同款）。
                                view()
                                    .w_full()
                                    .h(px(36.0))
                                    .flex_row()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.0))
                                    .border_1()
                                    .border_color(move || theme().danger)
                                    .key("login-logout")
                                    .on_press(move |_: &()| {
                                        if !confirm_logout.get() {
                                            confirm_logout.set(true);
                                            return;
                                        }
                                        confirm_logout.set(false);
                                        do_logout(message);
                                    })
                                    .child(
                                        text(move || {
                                            if confirm_logout.get() {
                                                t("common.confirm")
                                            } else {
                                                t("settings.logout")
                                            }
                                        })
                                        .text_size(px(13.0))
                                        .text_color(move || theme().danger),
                                    ),
                            )
                            .into_element()
                    },
                ))
                // 未登录：手机号 / 验证码表单 + 登录。
                .child(If::new(
                    move || !logged_in().get(),
                    move || {
                        let (mobile_row, code_row, mfa) = (mobile, code, mfa_retry);
                        view()
                            .w_full()
                            .flex_col()
                            .gap(px(12.0))
                            .child(
                                text(move || t("settings.loginPrompt"))
                                    .text_size(px(13.0))
                                    .text_color(theme().muted_foreground),
                            )
                            .child(
                                text(move || t("settings.loginMobilePlaceholder"))
                                    .text_size(px(13.0))
                                    .text_color(theme().foreground),
                            )
                            .child(field(mobile_row, false))
                            .child(
                                text(move || t("settings.loginCodePlaceholder"))
                                    .text_size(px(13.0))
                                    .text_color(theme().foreground),
                            )
                            .child(
                                view()
                                    .w_full()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child({
                                        let code_input = code_row;
                                        field(code_input, true).flex_1()
                                    })
                                    .child(
                                        view()
                                            .w(px(96.0))
                                            .h(px(36.0))
                                            .flex_row()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(8.0))
                                            .bg(theme().muted)
                                            .key("login-send-code")
                                            .on_press({
                                                let (mobile, busy, message) =
                                                    (mobile, busy, message);
                                                move |_: &()| send_code(mobile, busy, message)
                                            })
                                            .child(
                                                text(move || {
                                                    if busy.get() {
                                                        t("settings.loginSending")
                                                    } else {
                                                        t("settings.loginSendCode")
                                                    }
                                                })
                                                .text_size(px(13.0))
                                                .text_color(theme().foreground),
                                            ),
                                    ),
                            )
                            .child(
                                view()
                                    .w_full()
                                    .h(px(38.0))
                                    .flex_row()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.0))
                                    .bg(theme().primary)
                                    .key("login-submit")
                                    .on_press({
                                        let (mobile, code, busy, message) =
                                            (mobile, code_row, busy, message);
                                        move |_: &()| submit(mobile, code, busy, message, mfa_retry)
                                    })
                                    .child(
                                        text(move || {
                                            if mfa.with(|v| v.is_some()) {
                                                t("settings.loginRetry")
                                            } else {
                                                t("settings.loginAction")
                                            }
                                        })
                                        .text_size(px(14.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme().primary_foreground),
                                    ),
                            )
                            .child(
                                text(move || t("settings.loginRiskNote"))
                                    .text_size(px(12.0))
                                    .text_color(theme().muted_foreground),
                            )
                    },
                ))
                // 状态行（发码/登录/MFA 结果）。
                .child(If::new(
                    move || !msg_row.get().is_empty(),
                    move || {
                        text(move || msg_row.get())
                            .text_size(px(12.0))
                            .text_color(theme().muted_foreground)
                    },
                )),
        )
}

/// 一个输入框。
fn field(value: Signal<String>, is_code: bool) -> Input {
    input()
        .w_full()
        .h(px(36.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme().border)
        .bg(theme().background)
        .text_size(px(14.0))
        .text_color(theme().foreground)
        .placeholder(move || {
            t(if is_code {
                "settings.loginCodePlaceholder"
            } else {
                "settings.loginMobilePlaceholder"
            })
        })
        .placeholder_color(crate::tokens::icon_muted())
        .value(move || value.get())
        .max_length(20)
        .on_change_text(move |v: &str| value.set(v.to_owned()))
}

/// 发送验证码（csrf 凭据由 service 暂存，壳不经手）。
fn send_code(mobile: Signal<String>, busy: Signal<bool>, message: Signal<String>) {
    let number = mobile.get().trim().to_owned();
    if !valid_mobile(&number) || busy.get() {
        message.set(t("settings.loginInvalidMobile"));
        return;
    }
    busy.set(true);
    message.set(String::new());
    spawn(async move {
        let state = crate::state::state();
        let result = background(async move { flow::send_code(&state, &number).await }).await;
        busy.set(false);
        let text = match result {
            Ok(Ok(outcome)) => {
                let wait = if outcome.retry_time > 0 {
                    outcome.retry_time
                } else {
                    60
                };
                log::info!("[Login] 发码成功: {}", outcome.message);
                t("settings.codeSent").replace("{s}", &wait.to_string())
            }
            Ok(Err(e)) => {
                log::warn!("[Login] 发码失败: {e}");
                t("settings.loginSendFailed").replace("{message}", &e.to_string())
            }
            Err(e) => {
                log::warn!("[Login] 发码任务失败: {e}");
                t("settings.loginSendFailed").replace("{message}", &e.to_string())
            }
        };
        message.set(text);
    });
}

/// 提交登录：成功翻登录态；MFA 转后台轮询（后续经 pump 回来）。
fn submit(
    mobile: Signal<String>,
    code: Signal<String>,
    busy: Signal<bool>,
    message: Signal<String>,
    mfa_retry: Stored<Option<(String, String)>>,
) {
    let number = mobile.get().trim().to_owned();
    let otp = code.get().trim().to_owned();
    if !valid_mobile(&number) {
        message.set(t("settings.loginInvalidMobile"));
        return;
    }
    if otp.is_empty() || busy.get() {
        return;
    }
    busy.set(true);
    message.set(String::new());
    let retry = mfa_retry.with(|v| v.clone());
    spawn(async move {
        let state = crate::state::state();
        let result = background(async move {
            let mfa = retry
                .as_ref()
                .map(|(tag, key)| (tag.as_str(), key.as_str()));
            flow::submit(&state, &number, &otp, mfa).await
        })
        .await;
        busy.set(false);
        let text = match result {
            Ok(Ok(LoginResult::Success { user })) => {
                log::info!("[Login] 登录成功: {} ({})", user.name, user.user_id);
                set_logged_in(true);
                t("settings.loginSuccess").replace("{name}", &user.name)
            }
            Ok(Ok(LoginResult::Mfa {
                retry_tag,
                sms_code_key,
                tips,
                ..
            })) => {
                log::info!("[Login] 需要短信上行验证，转后台自动轮询");
                mfa_retry.replace(Some((retry_tag, sms_code_key)));
                tips
            }
            // 单次校验路径的结果；提交路径不会出现。
            Ok(Ok(LoginResult::MfaWaiting)) => String::new(),
            Ok(Err(e)) => {
                log::warn!("[Login] 登录失败: {e}");
                t("settings.loginFailed").replace("{message}", &e.to_string())
            }
            Err(e) => {
                log::warn!("[Login] 登录任务失败: {e}");
                t("settings.loginFailed").replace("{message}", &e.to_string())
            }
        };
        if !text.is_empty() {
            message.set(text);
        }
    });
}

/// 退出登录（纯本地操作）。账号区/面板两处共用。
pub fn do_logout(message: Signal<String>) {
    if let Err(e) = flow::logout(&crate::state::state()) {
        log::warn!("[Login] 退出失败: {e}");
        message.set(t("settings.logoutFailed").replace("{message}", &e.to_string()));
        return;
    }
    set_logged_in(false);
    log::info!("[Login] 已退出登录");
    message.set(t("settings.loggedOut"));
}

/// 手机号打码（138****1234；长度不足原样返回）。
pub(crate) fn mask_mobile(mobile: &str) -> String {
    let chars: Vec<char> = mobile.chars().collect();
    if chars.len() >= 11 {
        format!(
            "{}****{}",
            chars[..3].iter().collect::<String>(),
            chars[7..].iter().collect::<String>()
        )
    } else {
        mobile.to_owned()
    }
}
