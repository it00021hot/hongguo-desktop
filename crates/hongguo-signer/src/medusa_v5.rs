//! APK 7.4.3.32 Medusa v5 的外层封装。内部字段由当前请求和 SDK 会话提供。

use crate::aes_v3::AesV3;
use crate::medusa::{key_hash, xmxor};
use crate::primitives::{md5_raw, xor_bytes};

mod field13;
mod license;
mod request;
pub use field13::{Field13, Field13Error};
pub use license::License;
pub use request::{RequestContext, RequestEntropy, RequestSigner, SignError, advance_ladon};

pub const LICENSE_CHANNEL: &str = "646637794";
pub const SDK_VERSION: &str = "v04.10.03-ml-android";
pub const SDK_VERSION_CODE: u32 = 0x040a_0300;

const INIT: [u32; 4] = [0xbbb5_de72, 0xd463_8d5b, 0x92a5_f73b, 0x89f8_a1c9];
const COLUMNS: [[usize; 4]; 4] = [[2, 0, 3, 1], [1, 3, 2, 0], [3, 2, 1, 0], [2, 3, 1, 0]];
const ORDER: [[usize; 16]; 4] = [
    [0, 9, 14, 15, 4, 13, 2, 7, 8, 1, 6, 3, 12, 5, 10, 11],
    [0, 9, 14, 11, 4, 13, 2, 3, 8, 1, 6, 15, 12, 5, 10, 7],
    [0, 9, 14, 3, 4, 13, 2, 15, 8, 1, 6, 11, 12, 5, 10, 7],
    [0, 9, 14, 15, 4, 13, 2, 3, 8, 1, 6, 11, 12, 5, 10, 7],
];
const WIRE_BITS: [u32; 8] = [6, 5, 3, 4, 2, 7, 1, 0];
const COMPACT_BITS: [u32; 8] = [1, 7, 3, 6, 2, 0, 4, 5];
const COMPACT_PAYLOAD_BYTES: usize = 31;
const SPARSE_BYTES: usize = COMPACT_PAYLOAD_BYTES * 8;
const PACKED_OVERHEAD: usize = 19;

pub struct Envelope<'a> {
    pub message: &'a [u8],
    pub magic: [u8; 16],
    pub flags: [u8; 8],
    pub query_digest_head: u8,
    pub field13_head: u8,
    pub khronos: u32,
    pub random: u32,
    pub packed_random: u32,
}

/// 对已经编码的 v5 protobuf 生成 Medusa wire bytes。
pub fn seal(envelope: &Envelope<'_>, sign_key: &[u8; 32], sboxes: &[u8; 1024]) -> Option<Vec<u8>> {
    if envelope.message.len() < SPARSE_BYTES - PACKED_OVERHEAD {
        return None;
    }
    let mut packed = pack(envelope, sign_key);
    let cipher = compact_cipher(&packed, envelope.khronos, sign_key, sboxes);
    for (group, encrypted) in packed
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(&cipher[..COMPACT_PAYLOAD_BYTES])
    {
        for (slot, byte) in group.iter_mut().enumerate() {
            let wire = WIRE_BITS[slot];
            let bit = (encrypted >> COMPACT_BITS[slot]) & 1;
            *byte = (*byte & !(1 << wire)) | (bit << wire);
        }
    }

    let timestamp = envelope.khronos.to_le_bytes();
    let mut out = Vec::with_capacity(25 + packed.len());
    out.extend_from_slice(&(5 ^ envelope.khronos).to_le_bytes());
    out.extend(
        envelope
            .magic
            .iter()
            .zip(timestamp.iter().cycle())
            .map(|(byte, stamp)| byte ^ stamp),
    );
    out.extend_from_slice(&envelope.random.to_le_bytes()[..2]);
    out.extend_from_slice(&[0, 1, cipher[COMPACT_PAYLOAD_BYTES]]);
    out.extend_from_slice(&packed);
    Some(out)
}

fn pack(envelope: &Envelope<'_>, sign_key: &[u8; 32]) -> Vec<u8> {
    let (hash, mask) = key_hash(sign_key, envelope.random);
    let check = 0x1800_0001
        | (u32::from(envelope.query_digest_head & 63) << 14)
        | (u32::from(envelope.field13_head & 63) << 8);
    let transformed = xmxor(envelope.message, &hash);
    let mut packed = Vec::with_capacity(PACKED_OVERHEAD + transformed.len());
    packed.push(0xca);
    packed.extend_from_slice(&envelope.packed_random.to_le_bytes());
    packed.extend_from_slice(&check.to_le_bytes());
    packed.extend(
        envelope
            .flags
            .iter()
            .chain(&transformed)
            .rev()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[!index & 3]),
    );
    packed.extend_from_slice(&envelope.random.to_le_bytes()[2..]);
    packed
}

fn compact_cipher(
    packed: &[u8],
    khronos: u32,
    sign_key: &[u8; 32],
    sboxes: &[u8; 1024],
) -> [u8; 32] {
    let mut compact = [0; 32];
    for (group, byte) in packed
        .as_chunks::<8>()
        .0
        .iter()
        .zip(&mut compact[..COMPACT_PAYLOAD_BYTES])
    {
        for (slot, value) in group.iter().enumerate() {
            let bit = (value >> WIRE_BITS[slot]) & 1;
            *byte |= bit << COMPACT_BITS[slot];
        }
    }
    compact[COMPACT_PAYLOAD_BYTES] = 1;
    let variant = (khronos & 3) as usize;
    let start = variant * 256;
    let cipher = AesV3::with_parameters(
        &md5_raw(&sign_key[..16]),
        sboxes[start..start + 256].to_vec(),
        COLUMNS[variant],
        ORDER[variant],
        INIT[variant],
    );
    let first = cipher.block(&xor_bytes(&compact[..16], &md5_raw(&sign_key[16..])));
    let second = cipher.block(&xor_bytes(&compact[16..], &first));
    compact[..16].copy_from_slice(&first);
    compact[16..].copy_from_slice(&second);
    compact
}
