//! 编码名与四字码：接口 JSON 用 `bytevc2`，MP4 `stsd` / `frma` 用 `bvc2`。

/// `video_meta.codec_type` 与选流契约里的线名。
pub const WIRE_NAME: &str = "bytevc2";

/// 解密后 `stsd` / `frma` 四字码（librempeg `isom_tags`：`AV_CODEC_ID_BVC2 ← 'bvc2'`）。
pub const FOURCC: &str = "bvc2";

/// 配置记录 box（对照官方/CDN：`encv` 内为 `bv2C`，不是标准 `vvcC`）。
pub const CONFIG_BOX: &str = "bv2C";

/// 接口或探测字符串是否为 ByteVC2。
pub fn is_bytevc2(codec: &str) -> bool {
    let c = codec.trim();
    c.eq_ignore_ascii_case(WIRE_NAME) || is_bytevc2_fourcc(c)
}

/// MP4 四字码是否为 `bvc2`（含 `encv` 解密前经 `frma` 还原后的值）。
pub fn is_bytevc2_fourcc(fourcc: &str) -> bool {
    fourcc.trim().eq_ignore_ascii_case(FOURCC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_and_fourcc_are_recognized() {
        assert!(is_bytevc2("bytevc2"));
        assert!(is_bytevc2("ByteVC2"));
        assert!(is_bytevc2("bvc2"));
        assert!(!is_bytevc2("bytevc1"));
        assert!(!is_bytevc2("h264"));
        assert!(!is_bytevc2(""));
    }
}
