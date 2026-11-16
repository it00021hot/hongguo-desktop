//! passport form body 的组件转义，与签名查询串使用相同的保留字符集。

/// 组件转义：encodeURIComponent 语义，空格按 `%20`，`-_.!~*'()` 不转义——与 `signer::ticket::encode_component` 字符集完全一致，login 的 form body 过同一套服务端解析。
pub fn encode_component(s: &str) -> String {
    const UNRESERVED: &str = "-_.!~*'()";
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || UNRESERVED.contains(ch) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
