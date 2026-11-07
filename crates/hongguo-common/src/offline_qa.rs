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

/// 离线 QA 下，真实 host 不是 loopback 的 URL 不能发出。未开启时一律放行。
pub fn external_url_forbidden(url: &str) -> bool {
    enabled() && !is_loopback_url(url)
}

/// 按 RFC 3986 取 authority 里最后一个 `@` 之后的 host。userinfo 里的 localhost 不算主机。
pub fn request_host(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    let hostport = match authority.rfind('@') {
        Some(at) => &authority[at + 1..],
        None => authority,
    };
    if hostport.is_empty() {
        return None;
    }
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        let end = rest.find(']')?;
        &rest[..end]
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

pub fn is_loopback_url(url: &str) -> bool {
    request_host(url).is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
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
