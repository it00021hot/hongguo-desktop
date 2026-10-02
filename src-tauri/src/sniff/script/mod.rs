//! 嗅探脚本。
//!
//! 脚本在 webview 里执行，**直接 return 原生值**（数组/对象），不要
//! `JSON.stringify`——wry 会自己编码一次，再 stringify 就得多剥一层字符串壳。
//! **注意**：Windows 上 `eval_with_callback` 会吞掉异常，所以脚本内部
//! 必须自己 try/catch 并把错误作为字符串返回，不能让异常冒出去。

pub mod cards;
pub mod meta;

pub use cards::SNIFFER_JS;
pub use meta::{BROWSE_META_JS, TITLE_JS};
