//! 字节系（红果 / 番茄小说）App 接口的请求签名：JS 实现 1:1 移植，禁改写法、不做算法优化。
//!
//! **签完不能再动 url/body/query**（含 query 顺序与 URL 编码方式）；[`constants`] 常量是黑盒实测产物不能改；
//! 设备档案字段与 [`device::video_ua`] 必须成对使用。签名失配时服务端静默丢弃（HTTP 200 + 0 字节），排查先看字节数。

#![allow(
    clippy::manual_rotate,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::excessive_precision,
    clippy::identity_op,
    // MD5 轮函数参数表要与 JS 的 md5_v3_step 逐行对读，不拆结构体
    clippy::too_many_arguments
)]

pub mod aes_v3;
pub mod constants;
pub mod device;
pub mod helios;
pub mod medusa;
pub mod medusa_v5;
pub mod primitives;
pub mod protobuf;
pub mod ticket;
pub mod tt_hash;
pub mod xargus;
pub mod xgorgon;

// 统一出口：只导出真正按 `crate::signer::X` 引用的符号，其余走 `signer::子模块::X`
pub use device::{
    APP_ID, APP_VERSION_CODE, APP_VERSION_CODE_STR, APP_VERSION_NAME, CHANNEL_ID,
    REGISTER_FINGERPRINTS, REGISTER_VERSION_CODE, REGISTER_VERSION_CODE_STR, REGISTER_VERSION_NAME,
    RegisterFingerprint, VIDEO_REFERER, anonymous_profile, fingerprint_for_cdid, fresh_cdid,
    video_ua,
};
pub use helios::helios;
pub use medusa::build_medusa;
pub use primitives::md5_hex_upper;
pub use ticket::{API_ORIGIN, sign_request_with};
