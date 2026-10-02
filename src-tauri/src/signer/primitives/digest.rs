//! 摘要原语：MD5 与 SM3。
//!
//! JS 版优先用 `node:crypto` 的原生 SM3，Electron 28 内置的 Node 18 + OpenSSL 1.1.1
//! 没有 SM3，因此回退到纯 JS 实现（`sm3.js`）。两条路径结果一致。
//! Rust 侧统一用 `sm3` crate（RustCrypto），与纯 JS 实现同为 ITU-T SM3 标准算法，
//! 不存在宿主环境差异，因此不再需要 JS 那样的回退分支。

// sm3 0.3 依赖 digest 0.9，与 md-5 依赖的 digest 0.10 不是同一套 trait，
// 因此这里分别按各自版本引入，不要试图统一。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_known_vectors() {
        // RFC 1321 附录 A.5
        assert_eq!(md5_hex_upper(b""), "D41D8CD98F00B204E9800998ECF8427E");
        assert_eq!(md5_hex_upper(b"abc"), "900150983CD24FB0D6963F7D28E17F72");
        assert_eq!(
            md5_hex_upper(b"message digest"),
            "F96B697D7CB7938D525A2F31AAF161D0"
        );
    }

    #[test]
    fn sm3_known_vectors() {
        // GM/T 0004-2012 标准样例
        assert_eq!(
            hex::encode(sm3(b"abc")),
            "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0"
        );
        assert_eq!(
            hex::encode(sm3(b"abcd".repeat(16).as_slice())),
            "debe9ff92275b8a138604889c18e5a4d6fdb70e5387e5765293dcba39c0c5732"
        );
    }

    #[test]
    fn md5_raw_length() {
        assert_eq!(md5_raw(b"x").len(), 16);
        assert_eq!(sm3(b"x").len(), 32);
    }
}
