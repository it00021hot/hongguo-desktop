//! 登录域数据模型（登录结果 / MFA 上下行 / 发码结果 / passport 用户信息）。

/// 短信登录 / MFA 的结构化结果。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoginOutcome {
    /// 登录成功：会话 cookie + 服务端返回的用户信息。
    Success {
        /// `k=v; k=v`（可直接放进 `ApiEnv::cookie`）
        cookies: String,
        user: PassportUser,
        /// 响应头 `x-tt-token` 下发的长凭据（可空：个别响应不带）
        token: String,
    },
    /// 需要短信上行 MFA 二次验证（error_code=2046）：带着上下文轮询
    /// [`upsms_verify`]，`Registered` 后用 [`mfa_relogin`] 换取会话。
    Mfa(Box<MfaContext>),
}

/// MFA 上行短信验证的完整上下文（2026-10-04 hgplayer 1.1.3 真机抓包对齐）。
///
/// `retry_tag`/`sms_code_key` 来自 `data.biz_params`；`encrypt_uid` 与
/// `event_params`/`common_params` 是 upsms 轮询 body 的原料；`verify_ways`
/// 里 `mobile_up_sms_verify` 的通道号与回复内容用于 UI 提示。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MfaContext {
    pub retry_tag: String,
    pub sms_code_key: String,
    pub encrypt_uid: String,
    pub log_id: String,
    pub verify_reason: String,
    pub verify_scene: String,
    pub copywriting_key: String,
    pub diversion_tag: String,
    /// 上行短信通道号（如 9515211003）
    pub channel_mobile: String,
    /// 要回复的短信内容（如 "YZ"）
    pub sms_content: String,
    /// 验证提示文案
    pub tips: String,
    /// MFA 会话绑定 cookie（sms_login 触发 MFA 时 Set-Cookie 下发）。
    /// 轮询与重登请求都必须带，否则服务端不认这次验证（2026-10-05
    /// 用户真机踩坑：通道号对了仍永远 1045）。
    #[serde(skip_serializing)]
    pub mfa_token: String,
}

/// 进行中的 MFA 流程：上下文 + 原始登录要素（registered 后自动重登用）。
#[derive(Debug, Clone)]
pub struct MfaFlow {
    pub ctx: MfaContext,
    pub mobile: String,
    pub code: String,
}

/// 登录响应里的用户信息（字段按 fqnovel passport 惯例宽松解析）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PassportUser {
    pub user_id: String,
    pub name: String,
    #[serde(default)]
    pub mobile: String,
    /// 头像 URL（sms_login / user_info 响应的 data.avatar_url，无则空）
    #[serde(default)]
    pub avatar_url: String,
    /// 响应原文（JSON body 整体随身；红果号 biz_user_id 等长尾字段按需
    /// 从中读——user_info 有 60+ 字段只解析 4 个，为个别字段加解析路径
    /// 不如存原文）
    #[serde(default)]
    pub raw: String,
}

/// MFA 上行短信验证的轮询状态。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum UpsmsState {
    /// error_code=1045：用户还没回复短信，继续轮询
    Waiting,
    /// `data.registered=true`：MFA 通过——**还没有会话 cookie**，
    /// 要用 [`mfa_relogin`] 重发登录换取。`mfa_token` 是本响应
    /// Set-Cookie 下发的新 token，重登必须带上。
    Registered {
        #[serde(default)]
        ticket: String,
        #[serde(skip_serializing)]
        mfa_token: String,
    },
}

/// 发码结果。`mobile_ticket` 保留字段（1.1.2 时代形态）；1.1.3 实测
/// 会话绑定改走 `passport_csrf_token` Cookie（`csrf_cookie`，不序列化
/// 给前端，Rust 侧 AppState 暂存后在 sms_login 注入）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendCodeOutcome {
    pub message: String,
    #[serde(default)]
    pub mobile_ticket: String,
    /// 重发等待秒数（服务端 retry_time，一般 60）
    #[serde(default)]
    pub retry_time: u32,
    /// 发码响应 Set-Cookie 里的 `passport_csrf_token`（登录会话凭据）。
    #[serde(skip)]
    pub csrf_cookie: String,
}
