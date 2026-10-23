//! 红果桌面版 —— Tauri 壳入口。
//!
//! 装配（插件、状态、协议、command 挂载）在 `crates/hongguo-tauri`；本文件
//! 只提供 Tauri 运行上下文——`generate_context!` 必须与 tauri.conf.json /
//! capabilities / icons 同 crate（tauri-build 按 CARGO_MANIFEST_DIR 找它们），
//! 所以入口 crate 位于 `crates/hongguo-tauri-app`。

/// 运行应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    hongguo_tauri::prepare()
        .build(tauri::generate_context!())
        .expect("启动失败")
        .run(hongguo_tauri::on_run_event);
}
