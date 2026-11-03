//! 红果桌面版共用层：无域归属、跨 crate 共用的领域模型、统一错误、纯工具、跨端常量。
//!
//! 是所有上层 crate 的公共底座，必须保持叶子：不引入业务编排、IO 实现或 UI 依赖，也不依赖任何其他 hongguo-* crate（出现反向依赖需求先怀疑归属放错层）。

pub mod consts;
pub mod error;
pub mod model;
pub mod utils;

pub use error::{AppError, AppResult};
