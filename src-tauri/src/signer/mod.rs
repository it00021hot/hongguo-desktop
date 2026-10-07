//! 字节系（红果 / 番茄小说）App 接口的请求签名。
//!
//! 官方 App 接口要求每个请求携带五个由时间戳和请求内容共同导出的签名头。
//! 缺少或算错任何一个，服务端都会**静默丢弃**——返回 `HTTP 200` + `0 字节`，
//! 不报错、不给错误码。所以排查「接口没数据」时，第一件事是看字节数而不是状态码。
//!
//! 本模块是 JS 实现的 1:1 移植，**不做任何算法优化**。
//!
//! ## 产出的请求头
//!
//! | 头 | 算法 | 来源模块 |
//! |---|---|---|
//! | `x-gorgon` | MD5 摘要 + RC4 变种混淆 | [`xgorgon`] |
//! | `x-argus` | 自研 f13 哈希（三分支） | [`xargus`] |
//! | `x-ladon` | 时间戳的 base64 | [`ticket`] |
//! | `x-helios` | 34 轮扩散哈希 | [`medusa`] |
//! | `x-medusa` | protobuf + SM3 变换 + AES 变体 | [`medusa`] |
//!
//! ## 三条不能违反的约束
//!
//! 1. **签完不能再动 url 和 body。** 签名是「URL 查询串 + body 字节 + 时间戳」的
//!    联合函数，改任何一个字节（包括 query 参数顺序、URL 编码方式）都会失配。
//! 2. **常量不能改。** [`constants`] 里的值是黑盒实测产物，与服务端一一对应。
//! 3. **设备档案要整体替换。** [`device::video_device`] 的字段与
//!    [`device::VIDEO_UA`] 必须成对使用：Medusa 会把 `device_id` / `version_name`
//!    写进密文，服务端两边比对。
//!
//! ## 关于 clippy
//!
//! 本模块**刻意**保留 JS 原始的位运算写法（`(x << 3) | (x >> 5)` 而不是
//! `x.rotate_left(3)`）与传感器浮点字面量的全部精度。理由只有一个：
//! 出问题时必须能和 `hongguo.js` 逐行对读，形式上的改写会增加排查成本。
//! 算法正确性由 `medusa_tests.rs` / `xgorgon.rs` 里的黄金向量锁住，不依赖写法。

#![allow(
    clippy::manual_rotate,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::excessive_precision,
    clippy::identity_op,
    // MD5 轮函数的参数表就是 (kind, a, b, c, d, m, shift, round_const)，
    // 拆成结构体反而不如和 JS 的 md5_v3_step 逐行对读
    clippy::too_many_arguments
)]

pub mod aes_v3;
pub mod constants;
pub mod device;
pub mod helios;
pub mod medusa;
pub mod primitives;
pub mod protobuf;
pub mod ticket;
pub mod tt_hash;
pub mod xargus;
pub mod xgorgon;

// 统一出口：只导出 crate 内真正按 `crate::signer::X` 引用的符号，
// 其余一律走 `signer::子模块::X`，避免出现没人用的转发。
pub use device::{video_device, APP_ID, CHANNEL_ID, VIDEO_REFERER, VIDEO_UA};
pub use helios::helios;
pub use medusa::build_medusa;
pub use primitives::md5_hex_upper;
pub use ticket::{sign_request_with, API_ORIGIN};
