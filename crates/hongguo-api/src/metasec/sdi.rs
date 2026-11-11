//! SDI get_token 的应用级 wire 封装。

use std::io::{Read, Write};

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use hongguo_common::{AppError, AppResult};
use hongguo_signer::primitives::zigzag;
use hongguo_signer::protobuf::{WireValue, read_field, varint};

const MAX_PLAINTEXT: usize = 64 * 1024;
const REQUEST_MAGIC: u64 = 0x2020_0422 << 1;
const RESPONSE_MAGIC: u64 = 0x2012_0312 << 1;
const IV_TAIL: [u8; 4] = 0x2020_0427_u32.to_le_bytes();
const XTEA_DELTA: u32 = 0x9e37_79b9;

pub struct Codec<'a> {
    pub common_key: &'a [u8; 32],
    pub inner_key: [u8; 16],
}

pub struct Entropy {
    pub nonce: [u8; 4],
    pub frame_head: u8,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct SdiToken {
    pub value: String,
    pub status: i64,
}

impl Codec<'_> {
    pub fn encode_request(
        &self,
        plaintext: &[u8],
        millis: u64,
        entropy: Entropy,
    ) -> AppResult<Vec<u8>> {
        if plaintext.len() > MAX_PLAINTEXT || millis > u64::MAX / 2 {
            return Err(invalid_sdi());
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(plaintext).map_err(|_| invalid_sdi())?;
        let mut compressed = encoder.finish().map_err(|_| invalid_sdi())?;
        // SDI's DEFLATE wrapper emits the 32 KiB window with FLEVEL=0.
        if compressed.len() < 2 || compressed[0] != 0x78 {
            return Err(invalid_sdi());
        }
        compressed[1] = 0x01;
        let mut container = (plaintext.len() as u32).to_le_bytes().to_vec();
        container.extend_from_slice(&compressed);
        let ciphertext = self.encrypt(&container, entropy);
        let mut packet = Vec::new();
        append_number(&mut packet, 1, REQUEST_MAGIC);
        append_number(&mut packet, 2, 2);
        append_number(&mut packet, 3, 2);
        append_bytes(&mut packet, 4, &ciphertext);
        append_number(&mut packet, 5, millis * 2);
        Ok(packet)
    }

    pub fn decode_response(&self, packet: &[u8]) -> AppResult<SdiToken> {
        let ciphertext = response_ciphertext(packet)?;
        let plaintext = self
            .decrypt(ciphertext)
            .map_err(|_| AppError::Network("MetaSec SDI encrypted response is invalid".into()))?;
        let (token, status) = decode_token_fields(&plaintext)?;
        if !matches!(status, 200 | 202) {
            return Err(AppError::Network(format!(
                "MetaSec SDI token status {status}"
            )));
        }
        let value = token.ok_or_else(|| {
            AppError::Network(format!("MetaSec SDI token status {status} has no token"))
        })?;
        Ok(SdiToken {
            value: value.to_owned(),
            status,
        })
    }

    fn encrypt(&self, container: &[u8], entropy: Entropy) -> Vec<u8> {
        let padding = (8 - container.len() % 8) & 7;
        let start = 1 + (padding.wrapping_sub(1) & 3);
        let end = start + container.len();
        let mut frame = vec![0; container.len() + 8 + padding];
        frame[0] = entropy.frame_head & 0xf8 | padding as u8;
        frame[start..end].copy_from_slice(container);
        let offset = if start >= 3 { 1 } else { end };
        frame[offset..offset + 2].copy_from_slice(&crc16(container).to_be_bytes());
        Xtea::new(self.inner_key, entropy.nonce).encrypt_cbc(&mut frame);
        let mut outer = Vec::with_capacity(frame.len() + 21);
        // The native decoder verifies the full-byte XOR against the first XTEA byte.
        outer.push(frame[0] ^ 3);
        outer.extend(frame);
        outer.extend(entropy.nonce);
        aes_encrypt(outer, self.common_key)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> AppResult<Vec<u8>> {
        let outer = aes_decrypt(ciphertext, self.common_key)?;
        if outer.len() < 13 || (outer.len() - 5) % 8 != 0 || outer[0] ^ outer[1] != 3 {
            return Err(invalid_sdi());
        }
        let last = outer.len() - 4;
        let nonce = outer[last..].try_into().map_err(|_| invalid_sdi())?;
        let mut frame = outer[1..last].to_vec();
        Xtea::new(self.inner_key, nonce).decrypt_cbc(&mut frame);
        inflate_frame(&frame)
    }
}

fn response_ciphertext(mut packet: &[u8]) -> AppResult<&[u8]> {
    let mut magic = None;
    let mut version = None;
    let mut cipher = None;
    let mut status = None;
    while !packet.is_empty() {
        match read_field(&mut packet).ok_or_else(invalid_sdi)? {
            (1, WireValue::Varint(value)) if magic.is_none_or(|previous| previous == value) => {
                magic = Some(value)
            }
            (2, WireValue::Varint(value)) if version.is_none_or(|previous| previous == value) => {
                version = Some(value)
            }
            (3, WireValue::Varint(value)) => status = Some(value),
            (6, WireValue::Bytes(value)) if cipher.is_none() => cipher = Some(value),
            (1 | 2 | 6, _) => return Err(invalid_sdi()),
            _ => {}
        }
    }
    if magic != Some(RESPONSE_MAGIC) || version != Some(2) {
        return Err(AppError::Network(
            "MetaSec SDI response envelope is invalid".into(),
        ));
    }
    cipher.ok_or_else(|| match status {
        Some(code) => AppError::Network(format!(
            "MetaSec SDI refused token exchange ({})",
            (code >> 1) as i64 ^ -((code & 1) as i64)
        )),
        None => AppError::Network("MetaSec SDI response has no token field".into()),
    })
}

fn decode_token_fields(mut plaintext: &[u8]) -> AppResult<(Option<&str>, i64)> {
    let mut token = None;
    let mut status = None;
    while !plaintext.is_empty() {
        match read_field(&mut plaintext).ok_or_else(invalid_sdi)? {
            (1, WireValue::Bytes(value)) if token.is_none() => {
                if value.len() > 256 {
                    return Err(invalid_sdi());
                }
                token = Some(std::str::from_utf8(value).map_err(|_| invalid_sdi())?);
            }
            (2, WireValue::Varint(value)) if status.is_none() => {
                status = Some((value >> 1) as i64 ^ -((value & 1) as i64));
            }
            (1 | 2, _) => return Err(invalid_sdi()),
            _ => {}
        }
    }
    Ok((
        token.filter(|value| !value.is_empty()),
        status
            .ok_or_else(|| AppError::Network("MetaSec SDI response has no token status".into()))?,
    ))
}

fn inflate_frame(frame: &[u8]) -> AppResult<Vec<u8>> {
    let padding = usize::from(frame[0] & 7);
    let start = 1 + (padding.wrapping_sub(1) & 3);
    let length = frame
        .len()
        .checked_sub(8 + padding)
        .ok_or_else(invalid_sdi)?;
    let container = frame.get(start..start + length).ok_or_else(invalid_sdi)?;
    let (expected, compressed) = container.split_at_checked(4).ok_or_else(invalid_sdi)?;
    let expected = u32::from_le_bytes(expected.try_into().map_err(|_| invalid_sdi())?) as usize;
    if expected > MAX_PLAINTEXT {
        return Err(invalid_sdi());
    }
    let mut plain = Vec::with_capacity(expected);
    ZlibDecoder::new(compressed)
        .take(expected as u64 + 1)
        .read_to_end(&mut plain)
        .map_err(|_| invalid_sdi())?;
    if plain.len() != expected {
        return Err(invalid_sdi());
    }
    Ok(plain)
}

struct Xtea {
    words: [u32; 4],
    rounds: u32,
    iv: [u8; 8],
}

impl Xtea {
    fn new(key: [u8; 16], nonce: [u8; 4]) -> Self {
        let words =
            std::array::from_fn(|i| u32::from_be_bytes(key[i * 4..i * 4 + 4].try_into().unwrap()));
        let rounds = 32 + 8 * (u32::from_le_bytes(nonce) % 5);
        let mut iv = [0; 8];
        iv[..4].copy_from_slice(&nonce);
        iv[4..].copy_from_slice(&IV_TAIL);
        Self { words, rounds, iv }
    }

    fn encrypt_cbc(&self, data: &mut [u8]) {
        let mut previous = self.iv;
        for block in data.as_chunks_mut::<8>().0 {
            block
                .iter_mut()
                .zip(previous)
                .for_each(|(byte, mask)| *byte ^= mask);
            let (mut left, mut right) = read_block(block);
            let mut sum: u32 = 0;
            for _ in 0..self.rounds {
                left = left.wrapping_add(self.mix(right, sum, sum));
                sum = sum.wrapping_add(XTEA_DELTA);
                right = right.wrapping_add(self.mix(left, sum, sum >> 11));
            }
            write_block(block, left, right);
            previous.copy_from_slice(block);
        }
    }

    fn decrypt_cbc(&self, data: &mut [u8]) {
        let mut previous = self.iv;
        for block in data.as_chunks_mut::<8>().0 {
            let cipher = *block;
            let (mut left, mut right) = read_block(block);
            let mut sum = self.rounds.wrapping_mul(XTEA_DELTA);
            for _ in 0..self.rounds {
                right = right.wrapping_sub(self.mix(left, sum, sum >> 11));
                sum = sum.wrapping_sub(XTEA_DELTA);
                left = left.wrapping_sub(self.mix(right, sum, sum));
            }
            write_block(block, left, right);
            block
                .iter_mut()
                .zip(previous)
                .for_each(|(byte, mask)| *byte ^= mask);
            previous = cipher;
        }
    }

    fn mix(&self, value: u32, sum: u32, index: u32) -> u32 {
        ((value << 4) ^ (value >> 5)).wrapping_add(value)
            ^ sum.wrapping_add(self.words[(index & 3) as usize])
    }
}

fn read_block(block: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes(block[..4].try_into().expect("8-byte block")),
        u32::from_be_bytes(block[4..].try_into().expect("8-byte block")),
    )
}

fn write_block(block: &mut [u8], left: u32, right: u32) {
    block[..4].copy_from_slice(&left.to_be_bytes());
    block[4..].copy_from_slice(&right.to_be_bytes());
}

fn aes_encrypt(mut data: Vec<u8>, key: &[u8; 32]) -> Vec<u8> {
    let (cipher, mut previous) = aes_parts(key);
    let padding = 16 - data.len() % 16;
    data.extend(std::iter::repeat_n(padding as u8, padding));
    for block in data.as_chunks_mut::<16>().0 {
        block
            .iter_mut()
            .zip(previous)
            .for_each(|(byte, mask)| *byte ^= mask);
        cipher.encrypt_block(block.into());
        previous.copy_from_slice(block);
    }
    data
}

fn aes_decrypt(data: &[u8], key: &[u8; 32]) -> AppResult<Vec<u8>> {
    if data.is_empty() || data.len() > MAX_PLAINTEXT * 2 || !data.len().is_multiple_of(16) {
        return Err(invalid_sdi());
    }
    let (cipher, mut previous) = aes_parts(key);
    let mut plaintext = data.to_vec();
    for (source, block) in data
        .as_chunks::<16>()
        .0
        .iter()
        .zip(plaintext.as_chunks_mut::<16>().0)
    {
        cipher.decrypt_block(block.into());
        block
            .iter_mut()
            .zip(previous)
            .for_each(|(byte, mask)| *byte ^= mask);
        previous.copy_from_slice(source);
    }
    let padding = usize::from(*plaintext.last().ok_or_else(invalid_sdi)?);
    if !(1..=16).contains(&padding)
        || plaintext
            .iter()
            .rev()
            .take(padding)
            .any(|byte| usize::from(*byte) != padding)
    {
        return Err(invalid_sdi());
    }
    plaintext.truncate(plaintext.len() - padding);
    Ok(plaintext)
}

fn aes_parts(key: &[u8; 32]) -> (Aes128, [u8; 16]) {
    let (key, iv) = key.split_at(16);
    (
        Aes128::new(key.try_into().expect("16-byte AES key")),
        iv.try_into().expect("16-byte AES IV"),
    )
}

pub(super) fn append_bytes(output: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    output.extend(varint(u64::from(field) << 3 | 2));
    output.extend(varint(bytes.len() as u64));
    output.extend_from_slice(bytes);
}

pub(super) fn append_number(output: &mut Vec<u8>, field: u32, value: u64) {
    output.extend(varint(u64::from(field) << 3));
    output.extend(varint(value));
}

pub(super) fn append_signed(output: &mut Vec<u8>, field: u32, value: i64) {
    append_number(output, field, zigzag(value) as u64);
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            let high = crc & 0x8000 != 0;
            crc <<= 1;
            if high {
                crc ^= 0x1021;
            }
        }
    }
    crc
}

fn invalid_sdi() -> AppError {
    AppError::Network("MetaSec SDI packet is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_native_outer_check_byte() {
        let codec = Codec {
            common_key: &[0x52; 32],
            inner_key: [0x61; 16],
        };
        let plaintext = b"synthetic SDI device data";
        let packet = codec
            .encode_request(
                plaintext,
                1_000,
                Entropy {
                    nonce: [1, 2, 3, 4],
                    frame_head: 0xd0,
                },
            )
            .unwrap();
        let mut remaining = packet.as_slice();
        let ciphertext = loop {
            match read_field(&mut remaining).unwrap() {
                (4, WireValue::Bytes(value)) => break value,
                _ => continue,
            }
        };
        let outer = aes_decrypt(ciphertext, codec.common_key).unwrap();
        assert_eq!(outer[0], outer[1] ^ 3);
        assert_eq!(codec.decrypt(ciphertext).unwrap(), plaintext);
    }

    #[test]
    fn rejected_concatenated_response_reports_the_last_sdk_status() {
        let mut packet = Vec::new();
        for status in [-5, -15] {
            append_number(&mut packet, 1, RESPONSE_MAGIC);
            append_number(&mut packet, 2, 2);
            append_signed(&mut packet, 3, status);
        }
        assert!(matches!(
            response_ciphertext(&packet),
            Err(AppError::Network(message)) if message == "MetaSec SDI refused token exchange (-15)"
        ));
    }
}
