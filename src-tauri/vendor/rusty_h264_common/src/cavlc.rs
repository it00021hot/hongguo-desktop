//! CAVLC — Context-Adaptive Variable-Length Coding of residual blocks.
//!
//! This is the entropy coder that actually compresses: a 4×4 block of quantized
//! coefficients (in zig-zag scan order) is coded as `coeff_token` (count +
//! trailing ones), trailing-one signs, the remaining levels, `total_zeros`, and
//! per-coefficient `run_before`. The VLC tables below are the exact H.264 tables
//! (matching the reference decoders our output is validated against).
//!
//! [`encode_residual_block`] and [`decode_residual_block`] are an exact inverse
//! pair, parameterized by `nc` (the neighbor-derived context that selects the
//! `coeff_token` table) and `max_coeff` (16 for a full 4×4, 15 for an AC block,
//! 4 for chroma DC). Neighbor `nc` bookkeeping lives in the macroblock layer.

#[allow(unused_imports)]
use alloc::vec;
#[allow(unused_imports)]
use alloc::vec::Vec;

use crate::bit_reader::{Cursor, OutOfData};
use crate::{BitReader, BitWriter};

/// Zig-zag scan of a raster 4×4 block (full DC+AC), **unrolled** like openh264's
/// `WelsScan4x4DcAc` — constant indices, so no `ZIGZAG_4X4[i]` table read and no
/// per-element bounds check (the looped `block[ZIGZAG_4X4[i]]` form forces one).
#[inline]
pub fn scan_4x4_dcac(d: &[i32; 16]) -> [i32; 16] {
    [
        d[0], d[1], d[4], d[8], d[5], d[2], d[3], d[6], d[9], d[12], d[13], d[10], d[7], d[11],
        d[14], d[15],
    ]
}

/// Zig-zag scan of the 15 AC coefficients (skipping DC), unrolled like openh264's
/// `WelsScan4x4Ac` (`= ZIGZAG_4X4[1..]`).
#[inline]
pub fn scan_4x4_ac(d: &[i32; 16]) -> [i32; 15] {
    [
        d[1], d[4], d[8], d[5], d[2], d[3], d[6], d[9], d[12], d[13], d[10], d[7], d[11], d[14],
        d[15],
    ]
}

/// Inverse of [`scan_4x4_dcac`] (decoder): scatter scan-order coefficients back to
/// the raster 4×4 block, unrolled (no `ZIGZAG_4X4[i]` table read / bounds check).
#[inline]
pub fn un_scan_4x4_dcac(s: &[i32; 16]) -> [i32; 16] {
    let mut d = [0i32; 16];
    d[0] = s[0];
    d[1] = s[1];
    d[4] = s[2];
    d[8] = s[3];
    d[5] = s[4];
    d[2] = s[5];
    d[3] = s[6];
    d[6] = s[7];
    d[9] = s[8];
    d[12] = s[9];
    d[13] = s[10];
    d[10] = s[11];
    d[7] = s[12];
    d[11] = s[13];
    d[14] = s[14];
    d[15] = s[15];
    d
}

/// Inverse of [`scan_4x4_ac`] (decoder): scatter the 15 AC coefficients into the
/// existing block's raster positions (leaving DC `[0]` untouched). `s[0..15]` used.
#[inline]
pub fn un_scan_4x4_ac_into(s: &[i32], d: &mut [i32; 16]) {
    d[1] = s[0];
    d[4] = s[1];
    d[8] = s[2];
    d[5] = s[3];
    d[2] = s[4];
    d[3] = s[5];
    d[6] = s[6];
    d[9] = s[7];
    d[12] = s[8];
    d[13] = s[9];
    d[10] = s[10];
    d[7] = s[11];
    d[11] = s[12];
    d[14] = s[13];
    d[15] = s[14];
}

/// `coded_block_pattern` for Intra macroblocks (4:2:0), indexed by `codeNum`
/// (the `me(v)` mapping, spec Table 9-4). Maps code number → CBP value.
#[rustfmt::skip]
const CBP_INTRA: [u8; 48] = [
    47, 31, 15, 0, 23, 27, 29, 30, 7, 11, 13, 14, 39, 43, 45, 46,
    16, 3, 5, 10, 12, 19, 21, 26, 28, 35, 37, 42, 44, 1, 2, 4,
    8, 17, 18, 20, 24, 6, 9, 22, 25, 32, 33, 34, 36, 40, 38, 41,
];

/// `coded_block_pattern` for Inter macroblocks (4:2:0), indexed by `codeNum`.
#[rustfmt::skip]
const CBP_INTER: [u8; 48] = [
    0, 16, 1, 2, 4, 8, 32, 3, 5, 10, 12, 15, 47, 7, 11, 13,
    14, 6, 9, 31, 35, 37, 42, 44, 33, 34, 36, 40, 39, 43, 45, 46,
    17, 18, 20, 24, 19, 21, 26, 28, 23, 27, 29, 30, 22, 25, 38, 41,
];

/// Inverse of a `codeNum → cbp` table: `cbp → codeNum`, for a direct lookup in
/// the encoder instead of a 48-entry linear `.position()` scan per macroblock.
const fn invert_cbp(table: &[u8; 48]) -> [u8; 48] {
    let mut inv = [0u8; 48];
    let mut i = 0;
    while i < 48 {
        inv[table[i] as usize] = i as u8;
        i += 1;
    }
    inv
}
const INV_CBP_INTRA: [u8; 48] = invert_cbp(&CBP_INTRA);
const INV_CBP_INTER: [u8; 48] = invert_cbp(&CBP_INTER);

/// Decodes a `coded_block_pattern` (`me(v)`) for an Intra macroblock.
pub fn read_cbp_intra(r: &mut BitReader) -> Result<u32, OutOfData> {
    let code_num = r.read_ue()? as usize;
    // A code_num past the me(v) table (>= 48) is a corrupt stream: error like
    // every other unmatched VLC here, never silently decode "no coefficients".
    CBP_INTRA.get(code_num).map(|&v| v as u32).ok_or(OutOfData)
}

/// Encodes a `coded_block_pattern` (`me(v)`) for an Intra macroblock.
pub fn write_cbp_intra(w: &mut BitWriter, cbp: u32) {
    // `INV_CBP_INTRA` is `[u8; 48]` and a coded_block_pattern is 0..47.
    w.write_ue(INV_CBP_INTRA[(cbp as usize).min(47)] as u32);
}

/// Decodes a `coded_block_pattern` (`me(v)`) for an Inter macroblock.
pub fn read_cbp_inter(r: &mut BitReader) -> Result<u32, OutOfData> {
    let code_num = r.read_ue()? as usize;
    // See read_cbp_intra: out-of-table me(v) is an error, not cbp 0.
    CBP_INTER.get(code_num).map(|&v| v as u32).ok_or(OutOfData)
}

/// Encodes a `coded_block_pattern` (`me(v)`) for an Inter macroblock.
pub fn write_cbp_inter(w: &mut BitWriter, cbp: u32) {
    w.write_ue(INV_CBP_INTER[(cbp as usize).min(47)] as u32);
}

// ---- coeff_token, four nC tables. Index = TotalCoeff*4 + TrailingOnes. ----

#[rustfmt::skip]
const COEFF_TOKEN_LEN: [[u8; 68]; 4] = [
    [
        1,0,0,0,
        6,2,0,0,  8,6,3,0,  9,8,7,5,  10,9,8,6,
        11,10,9,7, 13,11,10,8, 13,13,11,9, 13,13,13,10,
        14,14,13,11, 14,14,14,13, 15,15,14,14, 15,15,15,14,
        16,15,15,15, 16,16,16,15, 16,16,16,16, 16,16,16,16,
    ],
    [
        2,0,0,0,
        6,2,0,0,  6,5,3,0,  7,6,6,4,  8,6,6,4,
        8,7,7,5,  9,8,8,6,  11,9,9,6,  11,11,11,7,
        12,11,11,9, 12,12,12,11, 12,12,12,11, 13,13,13,12,
        13,13,13,13, 13,14,13,13, 14,14,14,13, 14,14,14,14,
    ],
    [
        4,0,0,0,
        6,4,0,0,  6,5,4,0,  6,5,5,4,  7,5,5,4,
        7,5,5,4,  7,6,6,4,  7,6,6,4,  8,7,7,5,
        8,8,7,6,  9,8,8,7,  9,9,8,8,  9,9,9,8,
        10,9,9,9, 10,10,10,10, 10,10,10,10, 10,10,10,10,
    ],
    [
        6,0,0,0,
        6,6,0,0,  6,6,6,0,  6,6,6,6,  6,6,6,6,
        6,6,6,6,  6,6,6,6,  6,6,6,6,  6,6,6,6,
        6,6,6,6,  6,6,6,6,  6,6,6,6,  6,6,6,6,
        6,6,6,6,  6,6,6,6,  6,6,6,6,  6,6,6,6,
    ],
];

#[rustfmt::skip]
const COEFF_TOKEN_BITS: [[u8; 68]; 4] = [
    [
        1,0,0,0,
        5,1,0,0,  7,4,1,0,  7,6,5,3,  7,6,5,3,
        7,6,5,4,  15,6,5,4,  11,14,5,4,  8,10,13,4,
        15,14,9,4, 11,10,13,12, 15,14,9,12, 11,10,13,8,
        15,1,9,12, 11,14,13,8,  7,10,9,12,  4,6,5,8,
    ],
    [
        3,0,0,0,
        11,2,0,0,  7,7,3,0,  7,10,9,5,  7,6,5,4,
        4,6,5,6,  7,6,5,8,  15,6,5,4,  11,14,13,4,
        15,10,9,4, 11,14,13,12, 8,10,9,8, 15,14,13,12,
        11,10,9,12, 7,11,6,8,  9,8,10,1,  7,6,5,4,
    ],
    [
        15,0,0,0,
        15,14,0,0, 11,15,13,0, 8,12,14,12, 15,10,11,11,
        11,8,9,10, 9,14,13,9,  8,10,9,8, 15,14,13,13,
        11,14,10,12, 15,10,13,12, 11,14,9,12, 8,10,13,8,
        13,7,9,12,  9,12,11,10,  5,8,7,6,  1,4,3,2,
    ],
    [
        3,0,0,0,
        0,1,0,0,  4,5,6,0,  8,9,10,11,  12,13,14,15,
        16,17,18,19, 20,21,22,23, 24,25,26,27, 28,29,30,31,
        32,33,34,35, 36,37,38,39, 40,41,42,43, 44,45,46,47,
        48,49,50,51, 52,53,54,55, 56,57,58,59, 60,61,62,63,
    ],
];

/// chroma DC (2×2) coeff_token, index = TotalCoeff*4 + TrailingOnes.
#[rustfmt::skip]
const CHROMA_DC_COEFF_TOKEN_LEN: [u8; 20] = [
    2,0,0,0,  6,1,0,0,  6,6,3,0,  6,7,7,6,  6,8,8,7,
];
#[rustfmt::skip]
const CHROMA_DC_COEFF_TOKEN_BITS: [u8; 20] = [
    1,0,0,0,  7,1,0,0,  4,6,1,0,  3,3,2,5,  2,3,2,0,
];

/// total_zeros, indexed `[TotalCoeff-1][total_zeros]` (TotalCoeff 1..15).
#[rustfmt::skip]
const TOTAL_ZEROS_LEN: [[u8; 16]; 15] = [
    [1,3,3,4,4,5,5,6,6,7,7,8,8,9,9,9],
    [3,3,3,3,3,4,4,4,4,5,5,6,6,6,6,0],
    [4,3,3,3,4,4,3,3,4,5,5,6,5,6,0,0],
    [5,3,4,4,3,3,3,4,3,4,5,5,5,0,0,0],
    [4,4,4,3,3,3,3,3,4,5,4,5,0,0,0,0],
    [6,5,3,3,3,3,3,3,4,3,6,0,0,0,0,0],
    [6,5,3,3,3,2,3,4,3,6,0,0,0,0,0,0],
    [6,4,5,3,2,2,3,3,6,0,0,0,0,0,0,0],
    [6,6,4,2,2,3,2,5,0,0,0,0,0,0,0,0],
    [5,5,3,2,2,2,4,0,0,0,0,0,0,0,0,0],
    [4,4,3,3,1,3,0,0,0,0,0,0,0,0,0,0],
    [4,4,2,1,3,0,0,0,0,0,0,0,0,0,0,0],
    [3,3,1,2,0,0,0,0,0,0,0,0,0,0,0,0],
    [2,2,1,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
];
#[rustfmt::skip]
const TOTAL_ZEROS_BITS: [[u8; 16]; 15] = [
    [1,3,2,3,2,3,2,3,2,3,2,3,2,3,2,1],
    [7,6,5,4,3,5,4,3,2,3,2,3,2,1,0,0],
    [5,7,6,5,4,3,4,3,2,3,2,1,1,0,0,0],
    [3,7,5,4,6,5,4,3,3,2,2,1,0,0,0,0],
    [5,4,3,7,6,5,4,3,2,1,1,0,0,0,0,0],
    [1,1,7,6,5,4,3,2,1,1,0,0,0,0,0,0],
    [1,1,5,4,3,3,2,1,1,0,0,0,0,0,0,0],
    [1,1,1,3,3,2,2,1,0,0,0,0,0,0,0,0],
    [1,0,1,3,2,1,1,1,0,0,0,0,0,0,0,0],
    [1,0,1,3,2,1,1,0,0,0,0,0,0,0,0,0],
    [0,1,1,2,1,3,0,0,0,0,0,0,0,0,0,0],
    [0,1,1,1,1,0,0,0,0,0,0,0,0,0,0,0],
    [0,1,1,1,0,0,0,0,0,0,0,0,0,0,0,0],
    [0,1,1,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
];

/// chroma DC total_zeros, indexed `[TotalCoeff-1][total_zeros]` (TotalCoeff 1..3).
#[rustfmt::skip]
const CHROMA_DC_TOTAL_ZEROS_LEN: [[u8; 4]; 3] = [
    [1,2,3,3],
    [1,2,2,0],
    [1,1,0,0],
];
#[rustfmt::skip]
const CHROMA_DC_TOTAL_ZEROS_BITS: [[u8; 4]; 3] = [
    [1,1,1,0],
    [1,1,0,0],
    [1,0,0,0],
];

/// run_before, indexed `[min(zerosLeft,7)-1][run_before]`.
#[rustfmt::skip]
const RUN_LEN: [[u8; 15]; 7] = [
    [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [1,2,2,0,0,0,0,0,0,0,0,0,0,0,0],
    [2,2,2,2,0,0,0,0,0,0,0,0,0,0,0],
    [2,2,2,3,3,0,0,0,0,0,0,0,0,0,0],
    [2,2,3,3,3,3,0,0,0,0,0,0,0,0,0],
    [2,3,3,3,3,3,3,0,0,0,0,0,0,0,0],
    [3,3,3,3,3,3,3,4,5,6,7,8,9,10,11],
];
#[rustfmt::skip]
const RUN_BITS: [[u8; 15]; 7] = [
    [1,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [3,2,1,0,0,0,0,0,0,0,0,0,0,0,0],
    [3,2,1,1,0,0,0,0,0,0,0,0,0,0,0],
    [3,2,3,2,1,0,0,0,0,0,0,0,0,0,0],
    [3,0,1,3,2,5,4,0,0,0,0,0,0,0,0],
    [7,6,5,4,3,2,1,1,1,1,1,1,1,1,1],
];

/// Selects the coeff_token VLC table from the context `nc`. `nc == -1` is the
/// chroma-DC table (handled separately by the caller).
fn coeff_token_table(nc: i32) -> usize {
    if nc < 2 {
        0
    } else if nc < 4 {
        1
    } else if nc < 8 {
        2
    } else {
        3
    }
}

/// Writes a `(len, bits)` VLC codeword.
fn put(w: &mut BitWriter, len: u8, bits: u8) {
    debug_assert!(len > 0, "writing an undefined VLC entry");
    w.write_bits(bits as u32, len as u32);
}

/// `nc` -> coeff_token table (spec Table 9-5 column select). `nc` is 0..=16 from
/// the neighbour average; a 17-entry lookup replaces the three-compare chain
/// that ran on every luma block.
const NC_TABLE: [u8; 17] = [0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3];

/// A flat VLC lookup: `entry[peek(width)]` packs `(symbol << 5) | length`;
/// `length == 0` marks "no codeword" (corrupt input). The H.264 VLC tables are
/// prefix-free, so one peek + one index decodes any codeword.
///
/// Built at COMPILE TIME into `static`s: no `OnceLock` acquire per macroblock,
/// no `Vec` indirection per symbol, and the table address is an immediate.
pub struct Lut {
    width: u32,
    entry: &'static [u16],
}

const fn max_len(lens: &[u8]) -> u32 {
    let mut m = 0u8;
    let mut i = 0;
    while i < lens.len() {
        if lens[i] > m {
            m = lens[i];
        }
        i += 1;
    }
    m as u32
}

/// For each codeword of length `l` and value `v`, every `width`-bit peek whose
/// top `l` bits equal `v` (the range `[v<<(width-l), (v+1)<<(width-l))`) maps to
/// it. Codes are prefix-free, so the ranges never overlap.
const fn build_lut<const N: usize>(lens: &[u8], bits: &[u8]) -> [u16; N] {
    let width = max_len(lens);
    assert!(
        1usize << width == N,
        "LUT size must be 1 << max code length"
    );
    let mut e = [0u16; N];
    let mut i = 0;
    while i < lens.len() {
        let l = lens[i] as u32;
        if l != 0 {
            let base = (bits[i] as usize) << (width - l);
            let span = 1usize << (width - l);
            let packed = ((i as u16) << 5) | l as u16;
            let mut k = 0;
            while k < span {
                e[base + k] = packed;
                k += 1;
            }
        }
        i += 1;
    }
    e
}

macro_rules! lut_family {
    ($arr:ident : $n:literal, $lens:ident, $bits:ident, $($i:literal => $name:ident),* $(,)?) => {
        $( static $name: [u16; 1usize << max_len(&$lens[$i])] = build_lut(&$lens[$i], &$bits[$i]); )*
        static $arr: [Lut; $n] = [ $( Lut { width: max_len(&$lens[$i]), entry: &$name } ),* ];
    };
}
lut_family!(COEFF_TOKEN_LUT: 4, COEFF_TOKEN_LEN, COEFF_TOKEN_BITS, 0 => CT0, 1 => CT1, 2 => CT2, 3 => CT3);
lut_family!(TOTAL_ZEROS_LUT: 15, TOTAL_ZEROS_LEN, TOTAL_ZEROS_BITS,
    0 => TZ0, 1 => TZ1, 2 => TZ2, 3 => TZ3, 4 => TZ4, 5 => TZ5, 6 => TZ6, 7 => TZ7,
    8 => TZ8, 9 => TZ9, 10 => TZ10, 11 => TZ11, 12 => TZ12, 13 => TZ13, 14 => TZ14);
lut_family!(CDC_TOTAL_ZEROS_LUT: 3, CHROMA_DC_TOTAL_ZEROS_LEN, CHROMA_DC_TOTAL_ZEROS_BITS, 0 => CZ0, 1 => CZ1, 2 => CZ2);
lut_family!(RUN_BEFORE_LUT: 7, RUN_LEN, RUN_BITS, 0 => RB0, 1 => RB1, 2 => RB2, 3 => RB3, 4 => RB4, 5 => RB5, 6 => RB6);
static CDC_COEFF_TOKEN: [u16; 1usize << max_len(&CHROMA_DC_COEFF_TOKEN_LEN)] =
    build_lut(&CHROMA_DC_COEFF_TOKEN_LEN, &CHROMA_DC_COEFF_TOKEN_BITS);
static CDC_COEFF_TOKEN_LUT: Lut = Lut {
    width: max_len(&CHROMA_DC_COEFF_TOKEN_LEN),
    entry: &CDC_COEFF_TOKEN,
};

/// One VLC symbol off the cursor.
#[inline(always)]
fn lut_read(l: &Lut, c: &mut Cursor) -> Result<usize, OutOfData> {
    // `entry` is `1 << width` long and `peek(width)` cannot exceed that, but
    // LLVM cannot relate the two; a miss yields `packed == 0`, and `len == 0`
    // is ALREADY the corrupt-codeword path -- so index fallibly.
    let packed = l.entry.get(c.peek(l.width) as usize).copied().unwrap_or(0);
    let len = (packed & 0x1F) as u32;
    if len == 0 {
        return Err(OutOfData);
    }
    c.skip(len)?;
    Ok((packed >> 5) as usize)
}

/// Maps a signed level to its base `levelCode` (before the first-level offset).
fn level_to_code(level: i32) -> i32 {
    if level > 0 {
        (level << 1) - 2
    } else {
        (-level << 1) - 1
    }
}

/// Writes one `level_prefix`/`level_suffix` for `code` at the given suffix
/// length, the exact inverse of [`decode_residual_block`]'s level parsing.
///
/// For small codes the prefix is the unary part and the suffix is `suffix_length`
/// bits. Above that, `level_prefix` escapes to 15 with a 12-bit suffix; and for
/// codes too large for 12 bits, to the **extended escape** (`level_prefix ≥ 16`,
/// suffix `level_prefix − 3` bits), without which large levels — common at very
/// low QP — would silently truncate.
fn write_level(w: &mut BitWriter, code: i32, suffix_length: u32) {
    let code = code as u32;
    // Short forms with prefix < 15 — prefix+suffix PACKED into one write (the
    // value `(1<<suffixsize)|suffix` in `prefix+1+suffixsize` bits, leading zeros
    // implicit). Mirrors openh264's single `CAVLC_BS_WRITE` per level.
    if suffix_length == 0 {
        if code < 14 {
            w.write_bits(1, code + 1);
            return;
        } else if code < 30 {
            w.write_bits((1u32 << 4) | (code - 14), 14 + 1 + 4);
            return;
        }
    } else {
        let prefix = code >> suffix_length;
        if prefix < 15 {
            let suffix = code & ((1 << suffix_length) - 1);
            w.write_bits((1u32 << suffix_length) | suffix, prefix + 1 + suffix_length);
            return;
        }
    }
    // Prefix ≥ 15. `rem` is the value beyond the prefix-15 base; the decoder's
    // `+15` for the suffix_length-0 case makes both bases (30 and 15<<sl) align.
    let base = if suffix_length == 0 {
        30
    } else {
        15u32 << suffix_length
    };
    let rem = code - base;
    if rem < 4096 {
        put_zeros_one(w, 15);
        w.write_bits(rem, 12);
        return;
    }
    // Extended escape: grow the prefix until the suffix fits. For prefix `p`,
    // the suffix is `p − 3` bits and encodes `rem − (2^(p−3) − 4096)`.
    let mut p = 16u32;
    while rem > (1u32 << (p - 2)) - 4097 {
        p += 1;
    }
    put_zeros_one(w, p);
    w.write_bits(rem - ((1 << (p - 3)) - 4096), p - 3);
}

/// Writes `n` zero bits followed by a `1` (the unary `level_prefix`).
fn put_zeros_one(w: &mut BitWriter, n: u32) {
    // `n` zero bits then a `1` is just the value `1` written in `n + 1` bits — one
    // write, not `n + 1`. (The level-prefix unary code, emitted for every coeff.)
    if n < 32 {
        w.write_bits(1, n + 1);
    } else {
        w.write_bits(0, n - 31);
        w.write_bits(1, 32);
    }
}

/// Encodes a 4×4 residual block (`coeffs` in zig-zag scan order) as CAVLC and
/// returns `total_coeff` (the non-zero count), which callers reuse as the block's
/// `nnz` — saving a separate counting pass.
///
/// - `max_coeff`: 16 (full), 15 (AC), or 4 (chroma DC).
/// - `nc`: neighbor context; pass `-1` for chroma DC.
pub fn encode_residual_block(
    w: &mut BitWriter,
    coeffs: &[i32],
    max_coeff: usize,
    nc: i32,
) -> usize {
    // MEASUREMENT: scope disabled — at 600k+/200k+ calls its own rdtsc pair
    // was >50% of the bucket and inflated every enclosing stage.
    // let _g = crate::prof::scope(crate::prof::Stage::EncWrite);
    debug_assert!(coeffs.len() >= max_coeff);
    debug_assert!(max_coeff <= 16);
    let chroma_dc = nc == -1;

    // openh264 `CavlcParamCal`: ONE descending pass yields levels[] (high→low),
    // run[] (high→low), total_coeff, and total_zeros at once — no positions array,
    // no second/third pass. Bit-identical to the per-position derivation. Runs for
    // every coded 4×4 block, so the saved passes matter.
    let mut levels = [0i32; 16];
    let mut run_val = [0usize; 16];
    let mut total_coeff = 0usize;
    let mut total_zeros = 0usize;
    // Bind the coefficient run to `max_coeff` once: `idx` is derived from it,
    // so every `coeffs[idx as usize]` below then folds.
    let coeffs = &coeffs[..max_coeff.min(coeffs.len())];
    let mut idx = coeffs.len() as isize - 1;
    while idx >= 0 && coeffs.get(idx as usize) == Some(&0) {
        idx -= 1;
    }
    while idx >= 0 {
        let Some(&cv) = coeffs.get(idx as usize) else {
            break;
        };
        levels[total_coeff & 15] = cv;
        idx -= 1;
        let mut count_zero = 0usize;
        while idx >= 0 && coeffs.get(idx as usize) == Some(&0) {
            count_zero += 1;
            idx -= 1;
        }
        total_zeros += count_zero;
        run_val[total_coeff & 15] = count_zero;
        total_coeff += 1;
    }
    let levels_hi_lo = &levels[..total_coeff];

    // Trailing ones: leading ±1 entries of the high→low list, capped at 3.
    let mut trailing_ones = 0usize;
    for &lv in levels_hi_lo {
        if lv.abs() == 1 && trailing_ones < 3 {
            trailing_ones += 1;
        } else {
            break;
        }
    }

    // --- coeff_token + trailing-one signs, PACKED into one write (openh264:
    // `n += iTrailingOnes; iValue = (iValue << iTrailingOnes) + uiSign`) ---
    let tok_idx = total_coeff * 4 + trailing_ones;
    let (ct_len, ct_bits) = if chroma_dc {
        // `[u8; 20]` and `[[u8; 68]; 4]` respectively; `tok_idx` is
        // `total_coeff * 4 + trailing_ones` and cannot reach either bound.
        (
            CHROMA_DC_COEFF_TOKEN_LEN[tok_idx.min(19)],
            CHROMA_DC_COEFF_TOKEN_BITS[tok_idx.min(19)],
        )
    } else {
        let t = coeff_token_table(nc) & 3;
        (
            COEFF_TOKEN_LEN[t][tok_idx.min(67)],
            COEFF_TOKEN_BITS[t][tok_idx.min(67)],
        )
    };
    if total_coeff == 0 {
        put(w, ct_len, ct_bits);
        return 0;
    }
    // sign bits for the trailing ones (high→low): 1 = negative.
    let mut sign = 0u32;
    for &lv in levels_hi_lo.iter().take(trailing_ones) {
        sign = (sign << 1) | (lv < 0) as u32;
    }
    w.write_bits(
        ((ct_bits as u32) << trailing_ones) | sign,
        ct_len as u32 + trailing_ones as u32,
    );

    // --- remaining levels (high→low) ---
    let mut suffix_length = if total_coeff > 10 && trailing_ones < 3 {
        1
    } else {
        0
    };
    for (k, &lv) in levels_hi_lo.iter().enumerate().skip(trailing_ones) {
        let mut code = level_to_code(lv);
        if k == trailing_ones && trailing_ones < 3 {
            code -= 2;
        }
        write_level(w, code, suffix_length);
        if suffix_length == 0 {
            suffix_length = 1;
        }
        if lv.abs() > (3 << (suffix_length - 1)) && suffix_length < 6 {
            suffix_length += 1;
        }
    }

    // --- total_zeros (computed in the single pass above) ---
    if total_coeff < max_coeff {
        if chroma_dc {
            // `[[u8; 4]; 3]`: chroma-DC blocks hold at most four coefficients.
            let row = &CHROMA_DC_TOTAL_ZEROS_LEN[(total_coeff - 1).min(2)];
            let brow = &CHROMA_DC_TOTAL_ZEROS_BITS[(total_coeff - 1).min(2)];
            put(w, row[total_zeros & 3], brow[total_zeros & 3]);
        } else {
            put(
                w,
                // `[[u8; 16]; 15]`: `total_coeff` is 1..15 here, `total_zeros` 0..15.
                TOTAL_ZEROS_LEN[(total_coeff - 1).min(14)][total_zeros & 15],
                TOTAL_ZEROS_BITS[(total_coeff - 1).min(14)][total_zeros & 15],
            );
        }
    }

    // --- run_before (high→low), skipping once no zeros remain ---
    // run_val[] (high→low) came from the single pass above.
    let mut zeros_left = total_zeros;
    for &run in run_val[..total_coeff].iter().take(total_coeff - 1) {
        if zeros_left == 0 {
            break;
        }
        let t = zeros_left.min(7) - 1;
        put(
            w,
            RUN_LEN[t.min(6)][run.min(14)],
            RUN_BITS[t.min(6)][run.min(14)],
        );
        zeros_left -= run;
    }
    total_coeff
}

/// Reads a `level_prefix` (count of leading zeros before a `1`): one CLZ on the
/// 24-bit window when the codeword fits it, else exact bit-at-a-time.
#[inline(always)]
fn read_level_prefix(c: &mut Cursor) -> Result<u32, OutOfData> {
    let window = c.peek24();
    let lz = window.leading_zeros() - 8;
    if lz < 24 {
        c.skip(lz + 1)?;
        return Ok(lz);
    }
    let (n, c2) = read_level_prefix_long(*c)?;
    *c = c2;
    Ok(n)
}

#[cold]
#[inline(never)]
fn read_level_prefix_long(mut c: Cursor) -> Result<(u32, Cursor), OutOfData> {
    let mut n = 0;
    while !c.read_bit()? {
        n += 1;
        // A conformant 4x4 coefficient never needs a prefix this long; beyond
        // this the level computation (`1 << (prefix-3)`) would overflow, so a
        // longer run means corrupt input.
        if n > 32 {
            return Err(OutOfData);
        }
    }
    Ok((n, c))
}

/// Decodes a CAVLC residual block into `out` in zig-zag scan order, returning
/// `total_coeff`. `MAX` is the coefficient count of the block -- 16 (4x4 DC+AC),
/// 15 (AC-only) or 4 (chroma DC) -- and selects the chroma-DC tables at compile
/// time. **`out` must be all-zero on entry**: only the non-zero positions are
/// written, so an empty block costs one table lookup and nothing else.
///
/// Shape (2026-09-04 census -- the old form was a 528-instruction function
/// returning a 72-byte `Result` by pointer, with two out-of-line calls):
/// * the coeff_token and its trailing-one sign bits come from ONE 24-bit
///   window (token <= 16 bits + <= 3 signs);
/// * the `total_coeff == 0` case (36-49% of calls on the CAVLC corpus) returns
///   from this always-inlined head without entering the frame of the body;
/// * the body keeps the bit cursor in registers and commits it once;
/// * levels are placed as their `run_before` is read -- no `run_val` array, no
///   second pass: coefficient `k` (high->low) sits at `(tc-1-k) + zeros_below`.
#[inline(always)]
pub fn decode_residual_block_into<const MAX: usize, const N: usize>(
    r: &mut BitReader,
    nc: i32,
    out: &mut [i32; N],
) -> Result<u8, OutOfData> {
    // `N` is the OUTPUT length: 16 for the 4x4 categories, 4 for chroma DC, so a
    // DC block decodes straight into its 4-word slot instead of a 16-word scratch
    // that was zeroed and then copied (4 sites). `& (N - 1)` below is the own
    // bound of the array in both shapes.
    const { assert!(MAX == 16 || MAX == 15 || MAX == 4) };
    const { assert!((N == 16 || N == 4) && MAX <= N) };
    let _g = crate::prof::scope(crate::prof::Stage::Entropy);
    let mut c = r.cursor();
    // --- coeff_token (+ trailing-one signs) from one window ---
    let win = c.peek24();
    let lut: &Lut = if MAX == 4 {
        &CDC_COEFF_TOKEN_LUT
    } else {
        // `nc` is negative ONLY for chroma DC (the `MAX == 4` arm above, which never
        // reads this index), so the `max(0)` clamp was a dead cmp+cmov on every 4x4
        // block: a negative `nc as usize` still lands on `.min(16)`.
        &COEFF_TOKEN_LUT[NC_TABLE[(nc as usize).min(16)] as usize]
    };
    let packed = lut
        .entry
        .get((win >> (24 - lut.width)) as usize)
        .copied()
        .unwrap_or(0);
    let len = (packed & 0x1F) as u32;
    if len == 0 {
        return Err(OutOfData); // peeked bits matched no codeword -> corrupt
    }
    let idx = (packed >> 5) as usize;
    let (total_coeff, trailing_ones) = (idx >> 2, idx & 3);
    if total_coeff == 0 {
        c.skip(len)?;
        r.commit(c);
        return Ok(0);
    }
    // sign bits follow the token in the same window: 1 = negative.
    let signs = (win >> (24 - len - trailing_ones as u32)) & ((1u32 << trailing_ones) - 1);
    c.skip(len + trailing_ones as u32)?;
    // Scalars across the call boundary (see `Cursor::from_parts`).
    let (data, pos) = c.into_parts();
    let (tc, pos) = decode_coded_body::<MAX, N>(data, pos, total_coeff, trailing_ones, signs, out)?;
    r.commit(Cursor::from_parts(data, pos));
    Ok(tc)
}

/// The coded-block body: levels, total_zeros, run_before, placement.
// `drop(_lg)` ends the CavLvl scope early; with the `profile` feature off the
// guard is a ZST, which clippy flags as a no-op drop.
#[allow(clippy::drop_non_drop)]
#[inline(never)]
fn decode_coded_body<const MAX: usize, const N: usize>(
    data: &[u8],
    pos: usize,
    total_coeff: usize,
    trailing_ones: usize,
    signs: u32,
    out: &mut [i32; N],
) -> Result<(u8, usize), OutOfData> {
    // A block cannot hold more coefficients than it has positions (an AC block
    // decodes with the 16-coefficient table).
    let mut c = Cursor::from_parts(data, pos);
    if total_coeff > MAX {
        return Err(OutOfData);
    }

    // --- levels, high->low ---
    let _lg = crate::prof::scope(crate::prof::Stage::CavLvl);
    // Sized to the OUTPUT (`N` = 4 for chroma DC, 16 otherwise): `k < total_coeff
    // <= MAX <= N`, so `& (N - 1)` is the own bound, and a DC block no longer
    // zeroes a 64-byte level array to hold at most four values.
    let mut levels = [0i32; N];
    // Trailing-one signs, UNROLLED (the first sign read is the highest-frequency
    // coefficient, i.e. the MSB of `signs`). Left-aligning the up-to-3 bits
    // makes the three writes shift-free; LLVM had turned the 0..=3-trip loop
    // into a 60-instruction ymm sequence. Slots past `trailing_ones` are
    // rewritten by the level loop or never read.
    let s3 = signs << (3 - trailing_ones);
    levels[0] = 1 - 2 * ((s3 >> 2) & 1) as i32;
    levels[1] = 1 - 2 * ((s3 >> 1) & 1) as i32;
    levels[2] = 1 - 2 * (s3 & 1) as i32;
    let mut suffix_length = if total_coeff > 10 && trailing_ones < 3 {
        1
    } else {
        0
    };
    for k in trailing_ones..total_coeff {
        let level_prefix = read_level_prefix(&mut c)?;
        let level_suffix_size = if level_prefix == 14 && suffix_length == 0 {
            4
        } else if level_prefix >= 15 {
            level_prefix - 3
        } else {
            suffix_length
        };
        let level_suffix = if level_suffix_size > 0 {
            c.read_bits(level_suffix_size)?
        } else {
            0
        };
        let mut level_code = (level_prefix.min(15) << suffix_length) as i32 + level_suffix as i32;
        if level_prefix >= 15 && suffix_length == 0 {
            level_code += 15;
        }
        if level_prefix >= 16 {
            level_code += (1 << (level_prefix - 3)) - 4096;
        }
        if k == trailing_ones && trailing_ones < 3 {
            level_code += 2;
        }
        let level = if level_code % 2 == 0 {
            (level_code + 2) >> 1
        } else {
            (-level_code - 1) >> 1
        };
        // Residual coefficients are 16-bit (spec 8.5; ffmpeg stores int16). Only
        // the extended escape (prefix >= 16) can leave that range: prefix <= 15
        // bounds |level| by ((15<<6) + 4095 + 17) / 2 = 2529. So the check lives
        // in that arm alone -- same accept/reject set, three fewer ops per level.
        if level_prefix >= 16 && !(-32768..=32767).contains(&level) {
            return Err(OutOfData);
        }
        levels[k & (N - 1)] = level;
        if suffix_length == 0 {
            suffix_length = 1;
        }
        if level.abs() > (3 << (suffix_length - 1)) && suffix_length < 6 {
            suffix_length += 1;
        }
    }

    // --- total_zeros ---
    drop(_lg);
    let _rg = crate::prof::scope(crate::prof::Stage::CavRun);
    let total_zeros = if total_coeff < MAX {
        let l = if MAX == 4 {
            &CDC_TOTAL_ZEROS_LUT[(total_coeff - 1).min(2)]
        } else {
            &TOTAL_ZEROS_LUT[(total_coeff - 1).min(14)]
        };
        lut_read(l, &mut c)?
    } else {
        0
    };
    // The highest coefficient sits at total_coeff-1+total_zeros; past the block
    // is a corrupt stream (the per-position check this replaces rejected it).
    if total_coeff + total_zeros > MAX {
        return Err(OutOfData);
    }

    // --- run_before + placement (high->low) ---
    // `out` is `[i32; 16]`: with the bound above every position is <= 15, so
    // `& 15` is the own bound of the array -- a proof, not a relocation.
    let mut zeros_left = total_zeros;
    out[(total_coeff - 1 + total_zeros) & (N - 1)] = levels[0];
    for k in 1..total_coeff {
        if zeros_left > 0 {
            let run = lut_read(&RUN_BEFORE_LUT[zeros_left.min(7) - 1], &mut c)?;
            // A corrupt run_before may exceed the zeros remaining; reject rather
            // than underflow.
            zeros_left = zeros_left.checked_sub(run).ok_or(OutOfData)?;
        }
        out[((total_coeff - 1 - k) + zeros_left) & (N - 1)] = levels[k & (N - 1)];
    }
    Ok((total_coeff as u8, c.bit_pos()))
}

/// Decodes a CAVLC residual block into zig-zag-ordered coefficients. The first
/// `max_coeff` entries of the returned fixed array are valid (the rest stay zero).
/// Convenience form of [`decode_residual_block_into`] (tests, round-trips); the
/// decoder calls the const-generic form directly.
pub fn decode_residual_block(
    r: &mut BitReader,
    max_coeff: usize,
    nc: i32,
) -> Result<([i32; 16], u8), OutOfData> {
    let mut out = [0i32; 16];
    let total = match max_coeff {
        4 => decode_residual_block_into::<4, 16>(r, nc, &mut out)?,
        15 => decode_residual_block_into::<15, 16>(r, nc, &mut out)?,
        _ => decode_residual_block_into::<16, 16>(r, nc, &mut out)?,
    };
    Ok((out, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(block: &[i32], max_coeff: usize, nc: i32) {
        let mut w = BitWriter::new();
        encode_residual_block(&mut w, block, max_coeff, nc);
        w.align_zero();
        let bytes = w.into_bytes();
        let mut r = BitReader::new(&bytes);
        let decoded = decode_residual_block(&mut r, max_coeff, nc).expect("decode");
        let (dec_block, dec_total) = decoded;
        assert_eq!(
            &dec_block[..max_coeff],
            &block[..max_coeff],
            "nc={nc} max={max_coeff}"
        );
        assert_eq!(
            dec_total as usize,
            block[..max_coeff].iter().filter(|&&v| v != 0).count(),
            "returned total must equal the nonzero count (nc={nc})"
        );
    }

    #[test]
    fn all_zero_block() {
        roundtrip(&[0; 16], 16, 0);
        roundtrip(&[0; 16], 15, 0);
        roundtrip(&[0; 4], 4, -1);
    }

    #[test]
    fn single_dc() {
        let mut b = [0i32; 16];
        b[0] = 5;
        roundtrip(&b, 16, 0);
        b[0] = -3;
        roundtrip(&b, 16, 0);
    }

    #[test]
    fn trailing_ones_and_levels() {
        // DC=3, then some zeros, then ±1 trailing ones at higher frequency.
        let b = [3, 0, 1, -1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        roundtrip(&b, 16, 0);
    }

    #[test]
    fn many_coeffs_all_contexts() {
        let b = [1, -2, 3, -1, 1, 1, -1, 2, -3, 1, -1, 1, 1, -1, 1, -1];
        for nc in [0, 2, 4, 8, 20] {
            roundtrip(&b, 16, nc);
        }
    }

    #[test]
    fn chroma_dc_blocks() {
        roundtrip(&[2, -1, 0, 1], 4, -1);
        roundtrip(&[0, 0, 0, -1], 4, -1);
        roundtrip(&[1, 1, 1, 1], 4, -1);
    }

    #[test]
    fn large_levels_use_escape() {
        let b = [200, -150, 47, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        roundtrip(&b, 16, 0);
    }

    #[test]
    fn extreme_levels_use_extended_escape() {
        // Levels far beyond the 12-bit suffix range force level_prefix ≥ 16;
        // these occur at very low QP and previously truncated. Cover both
        // suffix_length==0 (single big DC) and grown-suffix_length (a run of
        // large levels) paths, and signs.
        roundtrip(&[5000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 16, 0);
        roundtrip(&[-7000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 16, 0);
        roundtrip(
            &[30000, -25000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            16,
            0,
        );
        let big = [
            9000, -9000, 8000, -8000, 7000, -7000, 6000, -6000, 5000, -5000, 4500, -4500, 4200,
            -4200, 4096, -4096,
        ];
        roundtrip(&big, 16, 0);
        // chroma DC and AC blocks with extreme levels too
        roundtrip(&[6000, -6000, 5000, -5000], 4, -1);
    }

    #[test]
    fn pseudo_random_blocks() {
        // Deterministic LCG; exercise many shapes across contexts and sizes.
        let mut state = 0x1234_5678u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state
        };
        for _ in 0..2000 {
            let mut b = [0i32; 16];
            let density = (next() % 16) as usize;
            for slot in b.iter_mut().take(density) {
                // small signed values, biased toward ±1
                let v = (next() % 7) as i32 - 3;
                *slot = v;
            }
            // shuffle into scan positions
            for i in (1..16).rev() {
                let j = (next() as usize) % (i + 1);
                b.swap(i, j);
            }
            let max_coeff = if next() % 3 == 0 { 15 } else { 16 };
            let nc = [0i32, 2, 4, 8][(next() % 4) as usize];
            roundtrip(&b, max_coeff, nc);
        }
    }
}
