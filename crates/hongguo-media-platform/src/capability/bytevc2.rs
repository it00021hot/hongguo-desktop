//! ByteVC2（`bvc2`）解码能力：纯 Rust 解帧编进二进制，不探测外部 ffmpeg。

/// 内置 ByteVC2 解码器是否启用。`HONGGUO_NO_BYTEVC2` 让选流排除 bytevc2，
/// 用来对照「仅 ByteVC1/HEVC」的既有路径。
pub fn bytevc2_decode_available() -> bool {
    if std::env::var_os("HONGGUO_NO_BYTEVC2").is_some() {
        log::info!("[Playback] HONGGUO_NO_BYTEVC2 已设置，本次按无 bytevc2 处理");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn rust_decoder_follows_the_opt_out_flag() {
        let enabled = super::bytevc2_decode_available();
        let opted_out = std::env::var_os("HONGGUO_NO_BYTEVC2").is_some();
        assert_eq!(enabled, !opted_out);
    }
}
