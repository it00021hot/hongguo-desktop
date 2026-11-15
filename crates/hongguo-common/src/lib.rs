//! 跨 crate 共用的领域模型、统一错误、文件 IO、纯工具与跨端常量。
//!
//! 不引入业务编排、UI 依赖或其他 hongguo-* crate。

pub mod consts;
pub mod error;
pub mod file_io;
pub mod media_url;
pub mod model;
pub mod utils;

pub use error::{AppError, AppResult};
