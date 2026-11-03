//! percent 编码，两套保留字符集**语义不同、不合并**：[`encode_component`] 不转义 `A-Za-z0-9 -_.!~*'()`（passport form body，与 signer::ticket 的 query 转义同字符集）；[`encode_rfc3986`] 仅 `A-Za-z0-9 -_.~*` 不转义（register 旧版 form 重放，RFC 3986 unreserved + `*`）。
//! 差异只在 `!'()` 四个字符：合并任一侧都会改变编码输出、与各自抓包形态失配（register 只剩重放测试在用，但形态必须锁死）。

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

/// RFC 3986 unreserved + `*`（register 旧版 form 重放字符集）：`!'()` 在这里**会**被转义，与 [`encode_component`] 的差异见模块注释。
pub fn encode_rfc3986(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_keeps_encode_uri_component_unreserved() {
        assert_eq!(encode_component("abc-_.!~*'()123"), "abc-_.!~*'()123");
        assert_eq!(encode_component("sms login"), "sms%20login", "空格按 %20");
        assert_eq!(
            encode_component(r#"{"a":"1","b":"x/y"}"#),
            "%7B%22a%22%3A%221%22%2C%22b%22%3A%22x%2Fy%22%7D"
        );
        assert_eq!(encode_component("剧"), "%E5%89%A7");
    }

    #[test]
    fn rfc3986_escapes_bang_quote_parens() {
        assert_eq!(encode_rfc3986("abc-_.~*123"), "abc-_.~*123");
        assert_eq!(
            encode_rfc3986("!'()"),
            "%21%27%28%29",
            "与 component 的差异字符"
        );
        assert_eq!(encode_rfc3986("a b"), "a%20b");
        assert_eq!(encode_rfc3986("剧"), "%E5%89%A7");
    }
}
