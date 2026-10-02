//! 字节 / 位运算原语（统一出口）。
//!
//! 按职责拆成四个文件，避免改一类算法时碰到另外三类：
//! - `bits`：位运算
//! - `bytes`：字节序与缓冲操作
//! - `digest`：MD5 / SM3
//! - `md5_variant`：变种 MD5（非标准算法）

// 子模块之间也要互相取用，因此对 crate 内开放
pub mod bits;
pub mod bytes;
mod digest;
mod md5_variant;

pub use bits::{reverse_bits, rol32, ror32, ror64, u32, zigzag};
pub use bytes::{be32, bxor, le32, read_u32_be, read_u32_le, xor_bytes};
pub use digest::{md5_hex_upper, md5_raw, sm3};
pub use md5_variant::{get_iv, sum_md5};
