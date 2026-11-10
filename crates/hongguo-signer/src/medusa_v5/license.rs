use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};

use crate::APP_ID;
use crate::primitives::md5_raw;
use crate::protobuf::{WireValue, read_field};

/// 应用级许可材料与设备会话分别持有。
pub struct License {
    sign_key: [u8; 32],
    common_key: [u8; 32],
}

impl License {
    pub fn decode(encoded: &str) -> Option<Self> {
        if encoded.len() != 304 {
            return None;
        }
        let mut data = STANDARD.decode(encoded).ok()?;
        let (prefix, encrypted) = data.split_at_mut(4);
        let key = md5_raw(&[APP_ID.as_bytes(), prefix, APP_ID.as_bytes()].concat());
        let cipher = Aes128::new((&key).into());
        let (blocks, remainder) = encrypted.as_chunks_mut::<16>();
        if !remainder.is_empty() {
            return None;
        }
        for block in blocks {
            cipher.decrypt_block(block.into());
        }
        let plain = unpad(encrypted)?;
        if plain.len() < 9 {
            return None;
        }
        let (head, rest) = plain.split_at(4);
        let (stream, tail) = rest.split_at(rest.len() - 4);
        let rc4_key = Sha1::digest([head, APP_ID.as_bytes(), prefix].concat());
        let mask = prefix
            .iter()
            .chain(head)
            .chain(tail)
            .fold(0, |acc, byte| acc ^ byte);
        let mut unpacked = rc4(stream, &rc4_key);
        unpacked.reverse();
        unpacked.iter_mut().for_each(|byte| *byte ^= mask);
        Self::keys(&unpacked)
    }

    pub fn sign_key(&self) -> &[u8; 32] {
        &self.sign_key
    }

    pub fn common_key(&self) -> &[u8; 32] {
        &self.common_key
    }

    fn keys(mut data: &[u8]) -> Option<Self> {
        let mut sign_key = None;
        let mut common_key = None;
        while !data.is_empty() {
            let (field, value) = read_field(&mut data)?;
            if let (8, WireValue::Bytes(entry)) = (field, value) {
                match key_entry(entry)? {
                    (b"sign_key", key) if sign_key.is_none() => sign_key = Some(key),
                    (b"common_key", key) if common_key.is_none() => common_key = Some(key),
                    _ => return None,
                }
            }
        }
        Some(Self {
            sign_key: sign_key?,
            common_key: common_key?,
        })
    }
}

fn key_entry(mut bytes: &[u8]) -> Option<(&[u8], [u8; 32])> {
    let mut name = None;
    let mut value = None;
    while !bytes.is_empty() {
        match read_field(&mut bytes)? {
            (1, WireValue::Bytes(data)) if name.is_none() => name = Some(data),
            (2, WireValue::Bytes(data)) if value.is_none() => value = Some(data),
            _ => return None,
        }
    }
    Some((name?, STANDARD.decode(value?).ok()?.try_into().ok()?))
}

fn unpad(input: &[u8]) -> Option<&[u8]> {
    let pad = usize::from(*input.last()?);
    if !(1..=16).contains(&pad) || pad > input.len() {
        return None;
    }
    let (plain, padding) = input.split_at(input.len() - pad);
    padding
        .iter()
        .all(|byte| usize::from(*byte) == pad)
        .then_some(plain)
}

fn rc4(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut state: [u8; 256] = std::array::from_fn(|index| index as u8);
    let mut j = 0u8;
    for index in 0..state.len() {
        j = j
            .wrapping_add(state[index])
            .wrapping_add(key[index % key.len()]);
        state.swap(index, usize::from(j));
    }
    j = 0;
    data.iter()
        .enumerate()
        .map(|(index, byte)| {
            let i = (index + 1) & 255;
            j = j.wrapping_add(state[i]);
            state.swap(i, usize::from(j));
            let select = state[i].wrapping_add(state[usize::from(j)]);
            byte ^ state[usize::from(select)]
        })
        .collect()
}
