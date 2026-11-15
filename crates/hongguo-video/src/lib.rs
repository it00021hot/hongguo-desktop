//! MP4 解析与 CENC 解密：box 树、样本表、CENC-AES-CTR 原语与解密管线
//! （`deprotect` 原位替换 / `decrypt_file` / `decrypt_buffer` / `streaming` 流式按 Range 惰性解）。

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
