//! 平台归档编码与 ByteVC2 解码能力。

pub mod bytevc2;
pub mod probe;

pub use bytevc2::bytevc2_decode_available;
pub use probe::selected_backend;
