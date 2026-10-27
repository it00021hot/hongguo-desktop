//! 崩溃取证（macOS）：SIGSEGV / SIGABRT 时把调用栈写进数据目录再退出。
//!
//! Rust 自带的栈溢出处理只打印一行「has overflowed its stack」，没有调用栈
//! ——深递归/栈泄漏类问题拿到那一行也无从定位。这里在**备用信号栈**（sigaltstack）
//! 上接住 SIGSEGV，用 `Backtrace` 抓栈后写盘：
//! `<数据目录>/crash.log`（追加；不按启动时间分文件，避免每次正常启动
//! 都在数据目录留一个空文件）。
//!
//! 文件在启动时就打开（信号上下文里不能再做路径解析/打开），处理器里只做
//! 追加写与 `_exit`。格式化 Backtrace 会分配到堆，这是取证与信号安全的
//! 取舍——崩溃现场本来就无法继续，宁可冒死锁风险也要留下栈。

use std::ffi::c_void;
use std::io::Write;
use std::sync::OnceLock;

#[repr(C)]
struct SigAction {
    sa_sigaction: usize,
    sa_mask: u32,
    sa_flags: u32,
}

#[repr(C)]
struct StackT {
    ss_sp: *mut c_void,
    ss_flags: i32,
    ss_size: usize,
}

const SIGSEGV: i32 = 11;
const SIGABRT: i32 = 6;
const SA_ONSTACK: u32 = 0x0001;
const SA_SIGINFO: u32 = 0x0004;
const ALT_SIZE: usize = 1 << 20;
const EXIT_CRASH: i32 = 134;

unsafe extern "C" {
    fn sigaction(sig: i32, act: *const SigAction, old: *mut SigAction) -> i32;
    fn sigaltstack(ss: *const StackT, old: *mut StackT) -> i32;
    fn _exit(code: i32) -> !;
}

static mut ALT: [u8; ALT_SIZE] = [0; ALT_SIZE];
/// 崩溃日志文件（启动时打开，信号处理器只追加写）。
static CRASH_FILE: OnceLock<std::fs::File> = OnceLock::new();

/// 安装崩溃取证（幂等；数据目录已就位后调用）。
pub(crate) fn install() {
    let dir = hongguo_store::paths::data_dir();
    let path = dir.join("crash.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            let _ = CRASH_FILE.set(file);
        }
        Err(e) => {
            log::warn!("[Crash] 崩溃日志打不开：{path:?}（{e}）");
            return;
        }
    }
    // SAFETY：安装期单线程；备用栈是进程级静态区
    unsafe {
        let alt = StackT {
            ss_sp: std::ptr::addr_of_mut!(ALT) as *mut c_void,
            ss_flags: 0,
            ss_size: ALT_SIZE,
        };
        sigaltstack(&alt, std::ptr::null_mut());

        let act = SigAction {
            // 经裸指针再转 usize：直接 fn→整数的 cast 被 clippy 拒（等价、显式）
            sa_sigaction: handler as *const () as usize,
            sa_mask: 0,
            sa_flags: SA_ONSTACK | SA_SIGINFO,
        };
        sigaction(SIGSEGV, &act, std::ptr::null_mut());
        sigaction(SIGABRT, &act, std::ptr::null_mut());
    }
    log::info!("[Crash] 崩溃取证已就位：{path:?}");
}

extern "C" fn handler(sig: i32, _info: *mut c_void, _ctx: *mut c_void) {
    let bt = std::backtrace::Backtrace::force_capture();
    let text = format!(
        "\n=== 崩溃：signal {sig}（{}） ===\n{bt:?}\n",
        if sig == SIGSEGV { "SIGSEGV" } else { "SIGABRT" }
    );
    if let Some(file) = CRASH_FILE.get() {
        // `&File` 也实现 Write，不需要可变借用
        let mut file = file;
        let _ = file.write_all(text.as_bytes());
        let _ = file.sync_all();
    }
    // SAFETY：备用信号栈上退出，不跑 atexit（崩溃现场不再做清理）
    unsafe { _exit(EXIT_CRASH) }
}
