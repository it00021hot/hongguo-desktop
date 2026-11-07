//! 显式离线 QA。默认关闭，正式启动路径不读这个开关。
//! 打开后只拒绝对外请求，不改变「无可信状态不自动注册」和已有持久阻断。

/// 进程环境变量。值为任意非空即开启；不设则是正式行为。
pub const ENV: &str = "HONGGUO_OFFLINE_QA";

pub fn enabled() -> bool {
    std::env::var_os(ENV).is_some()
}

/// 离线 QA 下，非 loopback 的 URL 不能发出。未开启时一律放行。
pub fn external_url_forbidden(url: &str) -> bool {
    enabled() && !is_loopback(url)
}

fn is_loopback(url: &str) -> bool {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = if let Some(rest) = after_scheme.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        after_scheme
            .split(['/', '?', '#', ':'])
            .next()
            .unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
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
        assert!(!external_url_forbidden("http://127.0.0.1:9/x"));
        assert!(!external_url_forbidden("http://localhost/x"));
        assert!(!external_url_forbidden("http://[::1]:80/x"));
        match prev {
            Some(v) => unsafe { std::env::set_var(ENV, v) },
            None => unsafe { std::env::remove_var(ENV) },
        }
    }
}
