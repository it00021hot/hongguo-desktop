//! 字节系签名算法魔数常量。
//!
//! 来源：woshishiq1/drpys @ 22261ad  spider/js/红果果[短].js（GPL-3.0）
//! 本文件由该源文件的常量原样提取生成，未做任何数值改动。
//!
//! ⚠️ 这些值是黑盒实测产物，与服务端一一对应。任何一项被改写或"顺手优化"，
//!    产出的签名都会被服务端**静默丢弃**（表现为 HTTP 200 + 0 字节）。
//!    用 `cargo run --bin probe_api` 逐端点看字节数来验证，不要只看状态码。

use base64::Engine;

/// AES 变体使用的 S-Box（Medusa 路径），4 组 × 256 字节。
///
/// 解码自 JS 的 `Buffer.from('...', 'base64')`。
pub fn sbox() -> Vec<u8> {
    static SBOX_B64: &str = "+n0Ia5xZs0sEXznQOEqRmQBnpiCf9U2Ccybu3xhmgzOAAxn72f6uqqmwUsYL83klTni0NqxdGieeiNu9PGPsSRXBMB/cuFbUbM3KCUPINaPvHvSW0vwOcnuUhNHqRVpiAj/TEoE0K91+5ijypUYTATsh9mE3KSoN7Yyvv51cuyR2D3XkU4nhmI2xmmVwT1RMWKtub4sjxAcRDLrPoKSO2AU9FLLadMPX577Wf95IFj6FkKFVt3dCIsmGUC4X+WQxLJvxbRxEaOPpqJOXyzJX6+Vxaq3AzMfF/WAdoi1Hp+JRaV56zgpBtpWP97mH4DoGEIq1+FvV8LyS/3wvwugbQOwb2r26mJEMsiuDQTRn+wrYdrVGBVlhI3WQhyrjUBVMrLF5667llUcEaPCGPVGLD8qO5LlO8hKCvA7V9+8oJc9bXelqVQLhM76T5/WtnT45JKji+hdX0HoNCDDWuKON/QeaxB5uImSX0h2wv0VmP2zd2yeApxHcpsVS+MC2yFwAc2B7oBkTqsk1SEvTpM2fmfMQREBUfin0Bh+iq6EvPPavhWI2IX9e3yAas7Tm/3KEj2UmlFp36kN4x0rMLBRrxuh0U/zUHM4xcAMYjJY4MonxOl/X+alpt2M3WMI7w3HLnpIBigtNiJu7T21v4P6lSd5WFgntnMEt7oF92XzRLkJbTcGmXepE/UVOG6E/0YnhfS+q26utWcuxzpooyeD2cDlK1/8w9d28VzsRjbLuALbmGlp8+d7EzS6Au7lMpZ+ECMZvQmzwJ+eLOpxR+2chdUExp8ogQyq3v9l68rX4jCwjg0+PYKAEEzcU4wHFY2ZcdIHfWL1okD3SszT0GZMyKdZJrg1L2AeerB4tC0C4crp2EHGo5FYdSP7lwkeR2ocmnR+Ia8CYviUJlzOjhRZef9xuVOn3qcjow3fQgivsAmKKkg4+sA8F8/GWeDiGNhg8JM8KtFPMYWWkx5TVFX5t73kiNRJqjlIGVXtGZFCV4gzt0xcDopuZ6xz8r9Rzafpf9ywev8jh8592gHFIqpStZPuJxmDDMrNN0uBE3V+oscdoIzTJbRJ/t+sVvqnReJOgDJKk10fjisJwqyZBmnmn2BSFj8BvVtCMEbkuPOKdzw7eA11GPs04Qw8zWtkaZWwiO/wwpojqN6K0jY5RnNZA7vn4hPSul+nKCkVnVwQvg1zVxcSCtqORmB9KrJaBbssbCQivGJVJfVTt+hYxOtq4ZvWl8f4QAQZ0zGPffCgl9s6yT4vlvIdpu4YhBwA25wtQWZsc6GJYGWHyvSdeuh3mmUI9DSq13Fsp8C1MU3tqc04/df9LoTUXVXI5INOw/e8C7Hd+5CvbkMEFnnrUUmskEw==";
    base64::engine::general_purpose::STANDARD
        .decode(SBOX_B64)
        .expect("SBOX 常量必须是合法 base64")
}

/// 变种 MD5 第二分支的 64 个初始向量。
pub const BRANCH2_SV: [u32; 64] = [
    0xa7aefe20, 0x7149f1d6, 0x47e4ca07, 0xe9b58f67, 0x93b924de, 0xc614d0f5, 0x38afe0ef, 0xb2bbad73,
    0xe24444c3, 0x9d3aec9b, 0xdf7b37e4, 0xd8b16d40, 0xf8ac31b8, 0x76b9a90b, 0x31d833ee, 0x953fce64,
    0x353595a4, 0x4609c13b, 0x36925008, 0x8c6d0925, 0x5df5c177, 0x1cfbf52b, 0x8a4fa7f0, 0x114ca35e,
    0x8193f984, 0x7a7a8733, 0x316ab4d5, 0x3c20cfc9, 0xa6d84453, 0x3a18500c, 0x798ec47a, 0x97a76b28,
    0x66c4ff96, 0x51716443, 0xdd2fc3b, 0xb5696da7, 0xbbeb3ac5, 0x5c53d204, 0xd32608ce, 0x7279b9ec,
    0xf4188ecf, 0xf7d793db, 0x332cc491, 0xab76ae15, 0x9bebe727, 0x18a01384, 0x5be9f8a7, 0x5f90a754,
    0x39b663c0, 0x36673c83, 0x7c92f514, 0x9d7d94d7, 0xe2e8d9aa, 0x5f7e9ea9, 0x7abd4551, 0x569e05da,
    0x40a25632, 0x3df5a9a5, 0xbab37d80, 0x454286dc, 0x3f5d4e78, 0x3d7b75d, 0xb1fe4af7, 0xa5ab26a3,
];

/// 变种 MD5 第二分支的 512 字节消息调度顺序（64 组 × 8 字节）。
pub fn branch2_orders() -> Vec<u8> {
    static ORDERS_HEX: &str = "0f0704000908030a060b050d0e010c020f05080c0009020103070e060b0a0d04060500070c000a04080f010b0d09020e060b02050403080101070a000d0c090e0d070e0f0b0208030c0509010004060a0d0902060f0b0a040807000c0503010e0c090f07060f030e020d0405010b0a000c050a090e0802040407030f01060b000b0f0408020a0700090d06010e03050c0b060a0508020c03070f0e090d0001040906080f050800040a0b030d01020c0e090d0c0604070a03030f00080105020e01000d0f090a0b0e0402080703060c0501080a0c0f0905060b000304020e070d04080f000c0f0e0d0a01060207090503040205080d0b0a0606000e0f070c09030a08040f000b01060d0c0709030e05020a070b050f00020e0108030d0c0609040d070f08050f06040b0a0e0c090002030d0c020704010b0e0e08060f09050003";
    hex::decode(ORDERS_HEX).expect("BRANCH2_ORDERS 必须是合法 hex")
}

/// Medusa 签名主密钥。
pub fn medusa_sign_key() -> Vec<u8> {
    // JS 源串中间有一个空格（换行拼接痕迹），此处已去除
    hex::decode(
        "8ebdfa3806ecc5cee79423e6029ed82540bc2218bb7eae f71cb691f7aa8aa2f5".replace(' ', ""),
    )
    .expect("MEDUSA_SIGN_KEY 必须是合法 hex")
}

/// Medusa AES 变体加密密钥。
pub fn medusa_aes_key() -> [u8; 16] {
    let v = hex::decode("f1593376766ea98d34f31b057a9d5be4").expect("MEDUSA_AES_KEY 必须是合法 hex");
    let mut out = [0u8; 16];
    out.copy_from_slice(&v);
    out
}

/// Medusa AES 变体初始向量。
pub fn medusa_aes_iv() -> [u8; 16] {
    let v = hex::decode("1fe109a4125283f418de9e051a969e12").expect("MEDUSA_AES_IV 必须是合法 hex");
    let mut out = [0u8; 16];
    out.copy_from_slice(&v);
    out
}

/// X-Gorgon 写入签名体的固定 sdk 版本号。
pub const GORGON_SDK_VERSION: u32 = 67_503_104;

/// X-Gorgon 输出前缀（2 字节 hex "8404"）。
pub const GORGON_PREFIX: [u8; 2] = [0x84, 0x04];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sbox_is_1024_bytes() {
        // Medusa 按 khronos 低 2 位取 4 组共 1024 字节
        assert_eq!(sbox().len(), 1024);
    }

    #[test]
    fn branch2_orders_is_320_bytes() {
        // 与 JS 源常量一致：320 字节 = 5 组 × 64 字节
        // branch2 用 (ivV0 << 6, (ivV0 + 1) << 6) 取 64 字节一组
        assert_eq!(branch2_orders().len(), 320);
    }

    #[test]
    fn keys_have_expected_length() {
        assert_eq!(medusa_sign_key().len(), 32);
        assert_eq!(medusa_aes_key().len(), 16);
        assert_eq!(medusa_aes_iv().len(), 16);
    }

    #[test]
    fn gorgon_prefix_matches_js() {
        assert_eq!(hex::encode(GORGON_PREFIX), "8404");
        assert_eq!(GORGON_SDK_VERSION, 67503104);
    }
}
