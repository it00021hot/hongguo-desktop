//! 显式离线 QA。默认关闭，正式启动路径不读这个开关。
//! 只有环境变量精确等于 [`ON`] 才开启，与 Vite 注入前端的取值同一条规则。
//! 打开后拒绝真实 host 不是 loopback 的请求，不改变「无可信状态不自动注册」。
//! 读、写和删走 [`io`]：在系统调用前解析现存祖先，不按文件名枚举；Unix 叶打开带 O_NOFOLLOW。

mod io;

pub use io::{
    isolation_root, note_production_root, prepare_qa_io, qa_clear_for_replace, qa_create,
    qa_create_dir_all, qa_read, qa_read_dir, qa_read_to_string, qa_remove_dir_all, qa_remove_file,
    qa_rename, qa_write, validate_isolated_dir,
};

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
