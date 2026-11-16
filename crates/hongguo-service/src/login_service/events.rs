//! 登录事件载荷。
//!
//! 与前端 `loginMfaStateSchema`（`service/schema/login.ts`）是同一契约的
//! 两份副本，改字段同一 commit 改另一端。

use serde::Serialize;

/// MFA 后台轮询状态事件载荷（`login-mfa-state`），三态互斥，前端按 `state` 判别（discriminated union）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum LoginMfaStateEvent {
    /// 自动登录完成（带昵称）。
    #[serde(rename_all = "camelCase")]
    Success { name: String },
    /// 验证失败/超时：带 i18n key 与后端原文，文案由前端渲染
    /// （与 IPC 错误契约同一策略：有译文用译文，无译文回落原文）。
    #[serde(rename_all = "camelCase")]
    Failed {
        /// 前端 i18n key（`settings.loginMfaTimeout` / `error.*` 等）。
        message_kind: String,
        /// 后端原文（服务端/底层透传；超时这类纯业务态为空串）。
        message: String,
    },
    /// 新一轮上行短信验证：`server_tip` 是服务端下发的展示内容，原样透传。
    #[serde(rename_all = "camelCase")]
    Waiting { server_tip: String },
}
