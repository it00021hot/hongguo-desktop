//! 有界解释经过校验的 APK 7.4.3.32 F13 字节码。

use std::collections::BTreeMap;
use std::fmt;

use crate::primitives::{get_iv, sm3, sum_md5};

mod instruction;
mod memory;

use memory::Memory;

const IMAGE_SIZE: usize = 4_501_136;
const IMAGE_SM3: [u8; 32] = [
    0x78, 0xfa, 0x77, 0x1c, 0xe4, 0xa8, 0xe3, 0x7a, 0x9e, 0x67, 0x4f, 0x2e, 0xa4, 0xff, 0x05, 0x96,
    0x3c, 0xc4, 0x64, 0xce, 0x5e, 0xaf, 0x22, 0x80, 0x3a, 0xcc, 0xf0, 0xac, 0x54, 0xe7, 0x81, 0x02,
];
const ENTRY: u64 = 0xa6d20;
const OUTPUT_READY: u64 = 0xecc40;
const CALLBACK: u64 = 0x2abe04;
const RETURN: u64 = 0x900000;
const ARGUMENTS: u64 = 0x710000;
const OUTPUT: u64 = 0x730000;
const INPUT: u64 = 0x740000;
const TABLE_BYTES: u64 = 0x800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field13Error {
    UnsupportedImage,
    InvalidMemory,
    UnhandledInstruction { location: u64, word: u32 },
    UnhandledCallback { location: u64, function: u64 },
    StepLimit,
}

impl fmt::Display for Field13Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedImage => formatter.write_str("unsupported MetaSec image"),
            Self::InvalidMemory => formatter.write_str("invalid MetaSec VM memory access"),
            Self::UnhandledInstruction { location, word } => {
                write!(
                    formatter,
                    "unsupported MetaSec VM instruction {word:#x} at {location:#x}"
                )
            }
            Self::UnhandledCallback { location, function } => {
                write!(
                    formatter,
                    "unsupported MetaSec VM callback {function:#x} at {location:#x}"
                )
            }
            Self::StepLimit => formatter.write_str("MetaSec VM instruction limit reached"),
        }
    }
}

impl std::error::Error for Field13Error {}

type Result<T> = std::result::Result<T, Field13Error>;

pub struct Field13 {
    image: Vec<u8>,
    relocations: BTreeMap<u64, u64>,
}

impl Field13 {
    pub fn from_image(image: Vec<u8>) -> Result<Self> {
        if image.len() != IMAGE_SIZE || sm3(&image) != IMAGE_SM3 {
            return Err(Field13Error::UnsupportedImage);
        }
        let relocations = image[0x1d60..0x65708]
            .as_chunks::<24>()
            .0
            .iter()
            .map(|entry| {
                let address = u64::from_le_bytes(entry[..8].try_into().expect("ELF64 address"));
                let info = u64::from_le_bytes(entry[8..16].try_into().expect("ELF64 relocation"));
                let addend = u64::from_le_bytes(entry[16..].try_into().expect("ELF64 addend"));
                let value = if info == 0x403 {
                    addend
                } else {
                    0x8000_0000 + (info >> 32) * 0x100
                };
                (address, value)
            })
            .collect();
        Ok(Self { image, relocations })
    }

    pub fn sboxes(&self) -> &[u8; 1024] {
        self.image[0xffcf9..0x1000f9]
            .try_into()
            .expect("verified ELF bounds")
    }

    pub fn version_magic(&self) -> [u8; 16] {
        self.image[0x665f0..0x66600]
            .try_into()
            .expect("verified ELF bounds")
    }

    pub fn sdi_inner_key(&self) -> [u8; 16] {
        let mut key = [0; 16];
        key[..8].copy_from_slice(&self.image[0x66690..0x66698]);
        for (index, value) in key[8..].iter_mut().enumerate() {
            *value = self.image[0x9d08c + index] ^ self.image[0x9d118 + index];
        }
        key
    }

    pub fn unavailable_string(&self) -> &str {
        std::str::from_utf8(&self.image[0x66c44..0x66c4c]).expect("verified ELF string")
    }

    pub fn digest(&self, query: &[u8; 32], body: &[u8; 16], khronos: u32) -> Result<[u8; 20]> {
        let mut input = [0; 52];
        input[..32].copy_from_slice(query);
        input[32..48].copy_from_slice(body);
        input[48..].copy_from_slice(&khronos.to_le_bytes());
        let selector = get_iv(0x2023_0928, &input) & 15;
        self.digest_at(&input, selector, khronos)
    }

    fn digest_at(&self, input: &[u8; 52], selector: u32, khronos: u32) -> Result<[u8; 20]> {
        let mut vm = Vm::new(self, input, selector, khronos)?;
        vm.run(OUTPUT_READY, 200_000)?;
        let pointer = vm.mem.read(OUTPUT + 16, 8);
        let mut digest = [0; 20];
        vm.mem.copy_to(pointer, &mut digest[..16]);
        let checksum = sum_md5(&digest[..16]) | 0x20;
        digest[16..].copy_from_slice(&checksum.to_le_bytes());
        Ok(digest)
    }
}

struct Vm<'a> {
    mem: Memory<'a>,
    regs: [u64; 32],
    entry: u64,
    pc: u64,
    hi: u64,
    stopped: bool,
}

impl<'a> Vm<'a> {
    fn new(image: &'a Field13, input: &[u8; 52], selector: u32, khronos: u32) -> Result<Self> {
        let mut mem = Memory::new(image);
        for (slot, value) in [
            OUTPUT,
            INPUT,
            u64::from(selector),
            u64::from(khronos),
            CALLBACK,
        ]
        .into_iter()
        .enumerate()
        {
            mem.write(ARGUMENTS + slot as u64 * 8, value, 8)?;
        }
        mem.string(INPUT, input)?;
        let mut regs = [0; 32];
        regs[4..8].copy_from_slice(&[ARGUMENTS, 0x3d6830, 0x3d6ac0, CALLBACK]);
        regs[29] = 0x720000;
        regs[31] = RETURN;
        Ok(Self {
            mem,
            regs,
            entry: ENTRY,
            pc: ENTRY,
            hi: 0,
            stopped: false,
        })
    }

    fn run(&mut self, boundary: u64, limit: usize) -> Result<()> {
        for _ in 0..limit {
            if self.pc == boundary || (boundary == RETURN && self.stopped) {
                return Ok(());
            }
            if !(ENTRY..0xf1290).contains(&self.pc) {
                return Err(Field13Error::InvalidMemory);
            }
            self.step()?;
        }
        Err(Field13Error::StepLimit)
    }

    fn callback(&mut self, args: u64) -> Result<()> {
        let function = self.mem.read(args, 8);
        let first = self.mem.read(args + 8, 8);
        let second = self.mem.read(args + 16, 8);
        let third = self.mem.read(args + 24, 8);
        match function {
            0x17d5a4 => self.mem.decode_literal(first, second, third),
            0x26f2e8 => {
                let length = self.mem.read(args + 20, 4) as usize;
                if length > 256 {
                    return Err(Field13Error::InvalidMemory);
                }
                self.mem.string(first, &vec![second as u8; length])
            }
            0x8000_0b00 => self.mem.fill(first, second as u8, third),
            0x8000_7000 => {
                if third > self.mem.read(args + 32, 8) {
                    return Err(Field13Error::InvalidMemory);
                }
                self.mem.fill(first, second as u8, third)?;
                self.mem.write(args + 40, first, 8)
            }
            0x8000_0e00 | 0x8000_1600 => self.mem.copy(first, second, third),
            0x3a0840 => {
                self.mem.copy(first, second, third)?;
                self.mem.write(args + 32, first, 8)
            }
            0x22438c | 0x223fdc | 0x22441c => self.mem.write(args + 8, 0, 4),
            0x298054 => {
                self.mem.write(first, 0, 8)?;
                self.mem.write(first + 8, 0, 8)
            }
            0x26f52c => Ok(()),
            0x2ab038 | 0x2ab280 | 0x2ab4c8 | 0x2ab710 | 0x2ab958 => self.make_tables(function),
            _ => Err(Field13Error::UnhandledCallback {
                location: self.pc,
                function,
            }),
        }
    }

    fn make_tables(&mut self, function: u64) -> Result<()> {
        let (global, first, next, table) = match function {
            0x2ab038 => (0x4628b8, 0xedda0, 0xee450, 0x3d6c40),
            0x2ab280 => (0x462900, 0xee840, 0xeeef0, 0x3d6c70),
            0x2ab4c8 => (0x462948, 0xef2e0, 0xef990, 0x3d6ca0),
            0x2ab710 => (0x462990, 0xefd80, 0xf0400, 0x3d6cd0),
            0x2ab958 => (0x4629d8, 0xf07f0, 0xf0ea0, 0x3d6d00),
            _ => unreachable!("only lookup callbacks call make_tables"),
        };
        if self.mem.read(global, 8) != 0 {
            return Ok(());
        }
        let base = self.mem.alloc(8 * TABLE_BYTES)?;
        for index in 0..8 {
            self.mem
                .write(global + index * 8, base + index * TABLE_BYTES, 8)?;
        }
        self.run_table(first, base, 0, [table, table + 0x20])?;
        for index in 0..7 {
            let previous = base + index * TABLE_BYTES;
            self.run_table(next, previous, previous + TABLE_BYTES, [0, 0])?;
        }
        Ok(())
    }

    fn run_table(&mut self, entry: u64, first: u64, second: u64, tables: [u64; 2]) -> Result<()> {
        let saved = (self.regs, self.pc, self.entry, self.hi, self.stopped);
        let args = self.mem.alloc(32)?;
        self.mem.write(args, first, 8)?;
        self.mem.write(args + 8, second, 8)?;
        self.mem.write(args + 16, CALLBACK, 8)?;
        self.regs = [0; 32];
        self.regs[4..8].copy_from_slice(&[args, tables[0], tables[1], CALLBACK]);
        self.regs[29] = 0x880000;
        self.regs[31] = RETURN;
        self.entry = entry;
        self.pc = entry;
        self.hi = 0;
        self.stopped = false;
        let result = self.run(RETURN, 1_500_000);
        (self.regs, self.pc, self.entry, self.hi, self.stopped) = saved;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_image_is_rejected() {
        assert!(matches!(
            Field13::from_image(vec![0; IMAGE_SIZE]),
            Err(Field13Error::UnsupportedImage)
        ));
    }

    #[test]
    #[ignore = "needs a local copy of the supported APK's MetaSec ELF"]
    fn f13_matches_all_apk_selector_vectors() {
        let path =
            std::env::var_os("HONGGUO_METASEC_IMAGE").expect("path to the local MetaSec ELF");
        let image = Field13::from_image(std::fs::read(path).unwrap()).unwrap();
        let input = std::array::from_fn(|index| index as u8);
        let expected = [
            "f88bb1fc90fee51b8b641d5b1f827221",
            "7d4e6bb79db2f99025de3931dcdf123a",
            "3277bede20e8164aae1fec3823b0ad6b",
            "36f6eeea63a4dc95132150b312cd04b8",
            "7a4aeaf6237e7d5c11b423679125fd08",
            "17ddbbf3baf8c6c96733a982e4c5a973",
            "3f7c513c405d3d146c8b6cd848a5d4a1",
            "8eb23355039486cccc6db8556b84bc00",
            "417eb675b6a820e879ac31e038dd6cdc",
            "93062abf9db748d7c2a1b9d54ef35bb7",
            "9d4caa9675bbaeb5cd2746678485628e",
            "0bb492958176d142d2a6a5bb1d160a6a",
            "49618563326efc2c6cd909e740f58a79",
            "ffbe0707d66ceea51eb86840d5028e35",
            "521e15d5b5e034633cec693b3429bc41",
            "1de6612b796c5f6cafb9ff0857e5ee68",
        ];
        for (mode, prefix) in expected.into_iter().enumerate() {
            let digest = image
                .digest_at(&input, mode as u32, 1_700_000_000)
                .unwrap_or_else(|error| panic!("selector {mode}: {error}"));
            assert_eq!(hex::encode(&digest[..16]), prefix, "selector {mode}");
        }
    }
}
