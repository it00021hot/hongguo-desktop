//! Tauri 壳入口：装配都在 `crates/hongguo-tauri`；本 crate 只承载 `generate_context!`（须与 tauri.conf.json / capabilities / icons 同 crate）。

/// 运行应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    hongguo_tauri::prepare()
        .build(tauri::generate_context!())
        .expect("启动失败")
        .run(hongguo_tauri::on_run_event);
}
