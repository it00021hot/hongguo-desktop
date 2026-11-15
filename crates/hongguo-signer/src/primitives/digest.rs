//! 摘要原语：MD5 与 SM3。Rust 侧统一用 `sm3` crate（RustCrypto，ITU-T SM3 标准算法），
//! 无宿主环境差异，不需要 JS 版因 Electron 内置 OpenSSL 缺 SM3 而设的回退分支。

// sm3 0.3 依赖 digest 0.9，与 md-5 依赖的 digest 0.10 不是同一套 trait，按各自版本引入，不要试图统一
use md5::Digest as Md5Digest;
use sm3::digest::Digest as Sm3Digest;

/// MD5 原始摘要（16 字节）。
pub fn md5_raw(value: &[u8]) -> [u8; 16] {
    let mut hasher = md5::Md5::new();
    hasher.update(value);
    let out = hasher.finalize();
    let mut result = [0u8; 16];
    result.copy_from_slice(&out);
    result
}

/// MD5 大写十六进制串（用于 x-ss-stub 头）。
pub fn md5_hex_upper(value: &[u8]) -> String {
    hex::encode_upper(md5_raw(value))
}

/// SM3 摘要（32 字节）。
pub fn sm3(value: &[u8]) -> [u8; 32] {
    let mut hasher = sm3::Sm3::new();
    Sm3Digest::update(&mut hasher, value);
    let out = Sm3Digest::finalize(hasher);
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}
