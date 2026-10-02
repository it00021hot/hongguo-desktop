//! 官网 HTML 兜底数据源。
//!
//! 官方 App 接口不可用时（签名失效、上游改协议）走这条链路，
//! 保证「接口挂了应用还能用」。

pub mod extract;
pub mod fetch;
pub mod series_page;
