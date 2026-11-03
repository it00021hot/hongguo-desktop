//! 元测试辅助：递归收集目录下的生产态 `.rs` 源码，供「扫源码断言约定」的测试使用（命令层 / service 层禁裸 `tokio::spawn` 等）。
//!
//! 仅测试构建引入，生产树零残留。约定：跳过 `*_tests.rs` 兄弟测试文件（可能合法使用测试专用 runtime，且不经内联 `#[cfg(test)]` 段）；每个文件的内联测试段由调用方按 `#[cfg(test)]` 自行截断后比对。

use std::path::{Path, PathBuf};

/// 递归收集 `dir` 下所有 `.rs`（跳过 `*_tests.rs`），返回 (路径, 源码)。
pub fn collect_sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
            continue;
        }
        let is_rs = path.extension().and_then(|ext| ext.to_str()) == Some("rs");
        let is_test_file = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_tests.rs"));
        if is_rs
            && !is_test_file
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            out.push((path, text));
        }
    }
}
