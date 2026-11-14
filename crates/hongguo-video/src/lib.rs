//! MP4 解析与 CENC 解密：box 树、样本表、CENC-AES-CTR 原语与解密管线
//! （`deprotect` 原位替换 / `decrypt_file` / `decrypt_buffer` / `streaming` 流式按 Range 惰性解）。
//! `fixtures` 只在测试构建（feature `test-support`）里存在，供解析层测试共用同一份字节构造。

pub mod audio_view;
pub mod crypto;

pub mod r#box;
pub mod cenc_info;
pub mod chunk_map;
pub mod decrypt_buffer;
pub mod decrypt_file;
pub mod deprotect;
pub mod esds;
pub mod sample_table;
pub mod streaming;
pub mod timing;

// 测试辅助模块下游 crate 的测试也要用（构造 MP4 样本）：feature 显式开启——
// `#[cfg(test)]` 对下游非 test 编译不可见，不加 feature 只能让测试代码常驻生产构建
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;

#[cfg(any(test, feature = "test-support"))]
#[path = "streaming_tests.rs"]
pub mod streaming_tests;
