//! MP4 解析与解密（按职责拆分）。
//!
//! - [`r#box`]：box 树遍历
//! - [`sample_table`]：样本表与加密辅助信息
//! - [`chunk_map`]：chunk → 样本 偏移换算
//! - [`cenc_info`]：`saiz` / `senc` 加密辅助信息
//! - [`deprotect`]：摘掉加密标记，还原成普通可播放的 MP4
//! - [`decrypt_file`]：落盘解密
//! - [`decrypt_buffer`]：内存解密（在线播放用）
//!
//! `fixtures` 只在测试构建里存在：解析层的测试要造 MP4 时共用同一份字节构造。

pub mod r#box;
pub mod cenc_info;
pub mod chunk_map;
pub mod decrypt_buffer;
pub mod decrypt_file;
pub mod deprotect;
pub mod sample_table;
pub mod timing;

#[cfg(test)]
pub mod fixtures;
