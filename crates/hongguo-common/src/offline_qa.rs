//! 显式离线 QA。默认关闭，正式启动路径不读这个开关。
//! 只有环境变量精确等于 [`ON`] 才开启，与 Vite 注入前端的取值同一条规则。
//! 打开后拒绝真实 host 不是 loopback 的请求，不改变「无可信状态不自动注册」。

/// 进程环境变量。未设置、空串或其他值都是正式联网行为。
pub const ENV: &str = "HONGGUO_OFFLINE_QA";

/// 与前端 `import.meta.env.VITE_HONGGUO_OFFLINE_QA === "1"` 相同。
pub const ON: &str = "1";

pub fn enabled() -> bool {
    std::env::var(ENV).ok().as_deref() == Some(ON)
}

/// 离线 QA 下，解析后不是 loopback HTTP(S) 的地址不能发出。解析失败同样拒绝。
/// 解析器是 `url` crate，与 reqwest 发送前用的是同一套。
pub fn external_url_forbidden(raw: &str) -> bool {
    if !enabled() {
        return false;
    }
    match url::Url::parse(raw) {
        Ok(parsed) => !offline_target_allowed(&parsed),
        Err(_) => true,
    }
}

/// 已经由发送端解析好的 URL。重定向每一跳把 reqwest 的 `Url` 交进来，不再拆字符串。
pub fn parsed_url_forbidden(parsed: &url::Url) -> bool {
    enabled() && !offline_target_allowed(parsed)
}

fn offline_target_allowed(parsed: &url::Url) -> bool {
    matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some_and(is_loopback_host)
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(|ch| ch == '[' || ch == ']');
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_defaults_off_and_only_loopback_stays_allowed() {
        let prev = std::env::var_os(ENV);
        unsafe { std::env::remove_var(ENV) };
        assert!(!enabled());
        assert!(!external_url_forbidden(
            "https://api5-normal-lq.fqnovel.com/foo"
        ));
        unsafe { std::env::set_var(ENV, "1") };
        assert!(external_url_forbidden(
            "https://api5-normal-lq.fqnovel.com/foo"
        ));
        assert!(external_url_forbidden("https://example.com:443/a"));
        assert!(external_url_forbidden("http://localhost:80@example.com/a"));
        assert!(external_url_forbidden("http://127.0.0.1:80@example.com/a"));
        assert!(external_url_forbidden("http://127.0.0.2/a"));
        assert!(!external_url_forbidden("http://127.0.0.1:9/x"));
        assert!(!external_url_forbidden("http://localhost/x"));
        assert!(!external_url_forbidden("http://[::1]:80/x"));
        assert!(!external_url_forbidden("http://user:pass@127.0.0.1/a"));
        assert!(external_url_forbidden(
            "https://example.com\\@127.0.0.1/a.jpeg"
        ));
        assert!(external_url_forbidden(" https://example.com/a.jpeg"));
        assert!(external_url_forbidden("https:/example.com/a.jpeg"));
        assert!(external_url_forbidden("not a url"));
        unsafe { std::env::set_var(ENV, "0") };
        assert!(!enabled());
        unsafe { std::env::set_var(ENV, "") };
        assert!(!enabled());
        unsafe { std::env::set_var(ENV, "true") };
        assert!(!enabled());
        match prev {
            Some(v) => unsafe { std::env::set_var(ENV, v) },
            None => unsafe { std::env::remove_var(ENV) },
        }
    }
}
