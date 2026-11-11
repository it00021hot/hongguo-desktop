use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::APP_ID;
use crate::helios::assemble_helios_v5;
use crate::primitives::{md5_raw, sm3};
use crate::protobuf::{FieldType as Kind, FieldValue as Value, proto};
use crate::xgorgon::x_gorgon_v5;

use super::{
    Envelope, Field13, Field13Error, LICENSE_CHANNEL, License, SDK_VERSION, SDK_VERSION_CODE, seal,
};

/// MetaSec 将线程原值、请求时间和本次分配扰动混合为 32 位 Ladon。
pub fn advance_ladon(previous: u32, allocation_nonce: u64, khronos: u32) -> u32 {
    let input = allocation_nonce ^ crc64(LICENSE_CHANNEL.as_bytes()) ^ u64::from(khronos);
    previous ^ input.rotate_left(4) as u32
}

fn crc64(bytes: &[u8]) -> u64 {
    let mut value = 0u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0x95ac_9329_ac4b_c9b5 & 0u64.wrapping_sub(value & 1));
        }
    }
    value
}

pub struct RequestSigner {
    image: Field13,
    license: License,
}

pub struct RequestContext<'a> {
    pub device_id: &'a str,
    pub version_name: &'a str,
    pub token: Option<&'a str>,
    pub counters: &'a [u8],
    pub environment: &'a [u8],
    pub runtime_json: &'a str,
    pub flags: u64,
    pub ladon: u32,
}

pub struct RequestEntropy {
    pub message: u32,
    pub envelope: u32,
    pub packed: u32,
    pub gorgon: u16,
    pub helios: u32,
}

impl RequestEntropy {
    pub fn draw() -> Self {
        Self {
            message: rand::random(),
            envelope: rand::random(),
            packed: rand::random(),
            gorgon: rand::random(),
            helios: rand::random(),
        }
    }
}

#[derive(Debug)]
pub enum SignError {
    Field13(Field13Error),
    InvalidMessage,
}

impl std::fmt::Display for SignError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Field13(error) => error.fmt(formatter),
            Self::InvalidMessage => formatter.write_str("incomplete MetaSec signing context"),
        }
    }
}

impl std::error::Error for SignError {}

impl RequestSigner {
    pub fn new(image: Field13, license: License) -> Self {
        Self { image, license }
    }

    pub fn image(&self) -> &Field13 {
        &self.image
    }

    pub fn common_key(&self) -> &[u8; 32] {
        self.license.common_key()
    }

    /// 签名已完成编码的 query 和报文，不生成或修改 URL。
    pub fn factors(
        &self,
        query: &str,
        body: Option<&[u8]>,
        khronos: u32,
        context: &RequestContext<'_>,
        entropy: &RequestEntropy,
    ) -> Result<Vec<(String, String)>, SignError> {
        let query_hash = sm3(query.as_bytes());
        let body_hash = body.map(md5_raw).unwrap_or_default();
        let field13 = self
            .image
            .digest(&query_hash, &body_hash, khronos)
            .map_err(SignError::Field13)?;
        let flags = context.flags.to_le_bytes();
        let magic = self.image.version_magic();
        let message = encode(
            context,
            magic,
            khronos,
            entropy.message,
            field13,
            &query_hash,
        );
        let envelope = Envelope {
            message: &message,
            magic,
            flags,
            query_digest_head: query_hash[0],
            field13_head: field13[0],
            khronos,
            random: entropy.envelope,
            packed_random: entropy.packed,
        };
        let medusa = seal(&envelope, self.license.sign_key(), self.image.sboxes())
            .ok_or(SignError::InvalidMessage)?;
        Ok(vec![
            ("x-khronos".into(), khronos.to_string()),
            ("x-argus".into(), STANDARD.encode(khronos.to_le_bytes())),
            (
                "x-ladon".into(),
                STANDARD.encode(context.ladon.to_be_bytes()),
            ),
            (
                "x-gorgon".into(),
                x_gorgon_v5(query, body, khronos, entropy.gorgon, [flags[0], flags[1]]),
            ),
            (
                "x-helios".into(),
                assemble_helios_v5(khronos, entropy.helios),
            ),
            ("x-medusa".into(), STANDARD.encode(medusa)),
        ])
    }
}

fn encode(
    context: &RequestContext<'_>,
    magic: [u8; 16],
    time: u32,
    random: u32,
    f13: [u8; 20],
    query_hash: &[u8; 32],
) -> Vec<u8> {
    let bytes = |tag, value: &[u8]| (tag, Value::Bytes(value.to_vec()), Kind::Bytes);
    let text = |tag, value: &str| (tag, Value::Str(value.into()), Kind::Str);
    let number = |tag, value| (tag, Value::SInt(value), Kind::SInt);
    proto(&[
        bytes(1, &magic),
        number(2, 5),
        number(3, i64::from(random)),
        text(4, APP_ID),
        text(5, context.device_id),
        text(6, LICENSE_CHANNEL),
        text(7, context.version_name),
        text(8, SDK_VERSION),
        number(9, i64::from(SDK_VERSION_CODE)),
        bytes(10, &context.flags.to_le_bytes()),
        number(12, i64::from(time)),
        bytes(13, &f13),
        bytes(14, &query_hash[..6]),
        bytes(15, context.counters),
        text(16, context.token.unwrap_or_default()),
        number(17, i64::from(time)),
        text(20, "none"),
        number(21, 369),
        bytes(23, context.environment),
        text(24, context.runtime_json),
    ])
}
