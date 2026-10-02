//! 极简 protobuf 编码。
//!
//! Medusa 签名把设备指纹包成 protobuf message 再加密。只需要 wire format 的
//! 编码侧（不解析、不支持 packed repeated / map），因此这里不做完整实现。

use crate::signer::primitives::zigzag;

/// 字段值类型。
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// UTF-8 字符串，wire type 2
    Str,
    /// 原始字节，wire type 2
    Bytes,
    /// 嵌套 message，wire type 2
    Message,
    /// sint32/zigzag，wire type 0
    SInt,
    /// 固定 32 位浮点，wire type 5
    Float,
}

/// 单个待编码字段。
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Str(String),
    Bytes(Vec<u8>),
    SInt(i64),
    Float(f32),
}

impl FieldValue {
    fn wire_type(&self, ty: &FieldType) -> Option<u8> {
        match ty {
            FieldType::Str | FieldType::Bytes | FieldType::Message => Some(2),
            FieldType::SInt => Some(0),
            FieldType::Float => Some(5),
        }
    }

    /// JS 侧 `undefined / null / ''` 三种情况直接跳过该字段。
    fn is_skipped(&self) -> bool {
        match self {
            FieldValue::Str(s) => s.is_empty(),
            FieldValue::Bytes(b) => b.is_empty(),
            _ => false,
        }
    }
}

/// 变长整数（varint）。
pub fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    while value > 127 {
        out.push(((value & 127) | 128) as u8);
        value >>= 7;
    }
    out.push(value as u8);
    out
}

/// 编码单个字段。
///
/// `None` 表示该字段被跳过（空字符串 / 空字节 / 空 message）。
pub fn proto_field(tag: u32, value: &FieldValue, ty: &FieldType) -> Option<Vec<u8>> {
    if value.is_skipped() {
        return None;
    }
    let wt = value.wire_type(ty).expect("未知 wire type");
    let key = varint((u64::from(tag) << 3) | u64::from(wt));

    let mut out = key;
    match (value, ty) {
        (FieldValue::Str(s), _) => {
            let body = s.as_bytes();
            out.extend_from_slice(&varint(body.len() as u64));
            out.extend_from_slice(body);
        }
        (FieldValue::Bytes(b), _) => {
            out.extend_from_slice(&varint(b.len() as u64));
            out.extend_from_slice(b);
        }
        (FieldValue::SInt(v), FieldType::SInt) => {
            out.extend_from_slice(&varint(zigzag(*v) as u64));
        }
        (FieldValue::Float(f), FieldType::Float) => {
            out.extend_from_slice(&f.to_le_bytes());
        }
        // Message 由调用方先编码成 Bytes 再以 FieldType::Message 传入
        (FieldValue::SInt(v), _) => out.extend_from_slice(&varint(*v as u64)),
        (FieldValue::Float(f), _) => out.extend_from_slice(&f.to_le_bytes()),
    }
    Some(out)
}

/// 编码一组字段。
pub fn proto(fields: &[(u32, FieldValue, FieldType)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (tag, value, ty) in fields {
        if let Some(bytes) = proto_field(*tag, value, ty) {
            out.extend_from_slice(&bytes);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_encodes_like_js() {
        // JS BigInt 参考值
        assert_eq!(varint(0), vec![0x00]);
        assert_eq!(varint(1), vec![0x01]);
        assert_eq!(varint(127), vec![0x7f]);
        assert_eq!(varint(128), vec![0x80, 0x01]);
        assert_eq!(varint(300), vec![0xac, 0x02]);
    }

    #[test]
    fn sint_uses_zigzag() {
        // tag=1, sint, value=1 → key=0x08, zigzag(1)=2
        let got = proto_field(1, &FieldValue::SInt(1), &FieldType::SInt).unwrap();
        assert_eq!(got, vec![0x08, 0x02]);
        // value=-1 → zigzag = 1
        let got = proto_field(1, &FieldValue::SInt(-1), &FieldType::SInt).unwrap();
        assert_eq!(got, vec![0x08, 0x01]);
    }

    #[test]
    fn string_field_encodes_length_prefixed() {
        let got = proto_field(3, &FieldValue::Str("abc".into()), &FieldType::Str).unwrap();
        assert_eq!(got, vec![0x1a, 0x03, b'a', b'b', b'c']);
    }

    #[test]
    fn empty_values_are_skipped() {
        assert!(proto_field(1, &FieldValue::Str(String::new()), &FieldType::Str).is_none());
        assert!(proto_field(1, &FieldValue::Bytes(Vec::new()), &FieldType::Bytes).is_none());
    }

    #[test]
    fn sint_zero_is_not_skipped() {
        // JS: value === undefined/null/'' 才跳过，数字 0 必须编码
        let got = proto_field(7, &FieldValue::SInt(0), &FieldType::SInt);
        assert!(got.is_some());
        assert_eq!(got.unwrap(), vec![0x38, 0x00]);
    }
}
