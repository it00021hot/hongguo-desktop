//! 跨 crate 共用的常量。
//!
//! 准入标准：无业务域归属且被多个 crate 共享；域内阈值（编解码参数、播放窗口、签名魔数、API 参数）留在各自域，脱离域的常量没人能判断该不该改。

pub mod scheme;
pub mod units;
