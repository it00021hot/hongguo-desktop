//! 发码 / 登录会话的 csrf 凭据注入。

use crate::app_state::AppState;
use hongguo_api::client::ApiEnv;

/// 把暂存的 csrf 并进调用环境（覆盖同名 cookie 字段，缺失则追加）。发码响应下发的
/// `passport_csrf_token` 是登录会话凭据，sms_login / 轮询都必须带上，否则服务端不认这次会话。
pub fn with_csrf(state: &AppState, mut env: ApiEnv) -> ApiEnv {
    let Some(csrf) = state.login_csrf.read().clone() else {
        return env;
    };
    let base = env.cookie.unwrap_or_default();
    let mut fields: Vec<(String, String)> = base
        .split("; ")
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
    for key in ["passport_csrf_token", "passport_csrf_token_default"] {
        match fields.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = csrf.clone(),
            None => fields.push((key.to_string(), csrf.clone())),
        }
    }
    env.cookie = Some(
        fields
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; "),
    );
    env
}
