//! 无业务语义的纯工具层（P3-C9 抽取）。
//!
//! 准入标准：不依赖 AppState/领域类型、纯函数、可单测。各域重复实现
//! 自这里收敛——同一份逻辑只在本层存一份，域内旧副本一律删除。
//! 保持私有模块（lib.rs `mod utils;`），不对外暴露。

pub(crate) mod hex;
pub(crate) mod json;
pub(crate) mod time;
pub(crate) mod url;
