//! MSB-first bit writer with H.264 Exp-Golomb coding.
//!
//! H.264 packs syntax elements as a big-endian bitstream: the first bit written
//! lands in the most-significant position of the first byte. This writer
//! accumulates bits and exposes the Exp-Golomb (`ue`/`se`) and fixed-length
//! (`u`) codings the bitstream syntax is built from.

#[allow(unused_imports)]
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

/// A growable, MSB-first bit buffer.
///
/// Uses a **bit cache**: bits accumulate in the low `nbits` positions of a 64-bit
/// `cache` (the most-recently-written bit is least-significant within the valid
/// region), and whole bytes are flushed off the top. A multi-bit write is a shift
/// + OR + a short byte-flush loop — not one operation per bit. After every public
/// write the invariant `nbits < 8` and "bits above `nbits` are zero" hold.
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    /// Completed bytes.
    bytes: Vec<u8>,
    /// Pending bits, valid in the low `nbits` positions (zero above).
    cache: u64,
    /// Number of valid pending bits (0..=7 between writes).
    nbits: u32,
}

impl BitWriter {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a writer with `bytes` pre-reserved — for the per-frame slice writer,
    /// so the CAVLC hot loop never pays a `Vec` realloc mid-frame.
    pub fn with_capacity(bytes: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(bytes),
            cache: 0,
            nbits: 0,
        }
    }

    /// Resets to the empty state, keeping the byte allocation (scratch reuse —
    /// E2 round 2, inline-execution.md 11.12a).
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.cache = 0;
        self.nbits = 0;
    }

    /// Number of bits written so far.
    pub fn bit_len(&self) -> usize {
        self.bytes.len() * 8 + self.nbits as usize
    }

    /// `true` if the writer is on a byte boundary.
    pub fn is_byte_aligned(&self) -> bool {
        self.nbits % 8 == 0
    }

    /// Writes the low `n` bits of `value`, most-significant first (`u(n)`). `n` <= 32.
    ///
    /// Keeps up to 31 pending bits in `cache` and flushes a whole **u32 word** (4
    /// bytes) at a time, like openh264's `BsWriteBits` (`WRITE_BE_32`) — a quarter
    /// the `Vec` writes of byte-at-a-time flushing.
    #[inline]
    pub fn write_bits(&mut self, value: u32, n: u32) {
        debug_assert!(n <= 32, "write_bits supports up to 32 bits");
        if n == 0 {
            return;
        }
        let mask = (1u64 << n) - 1; // n<=32 so 1<<32 fits u64
        self.cache = (self.cache << n) | (value as u64 & mask);
        self.nbits += n;
        if self.nbits >= 32 {
            self.nbits -= 32;
            let word = (self.cache >> self.nbits) as u32;
            self.bytes.extend_from_slice(&word.to_be_bytes());
            self.cache &= (1u64 << self.nbits) - 1; // drop the flushed high bits
        }
    }

    /// Appends whole bytes to a byte-ALIGNED writer — the CABAC payload
    /// hand-off (E15 round 2, inline-execution.md 11.9). Drains any whole
    /// pending bytes from the cache (alignment means `nbits` is a multiple of
    /// 8, but up to 24 bits may legally be pending), then ONE
    /// `extend_from_slice` replaces the former per-byte `write_bits` loop —
    /// byte-identical output, O(payload) fewer calls per slice.
    pub fn write_aligned_bytes(&mut self, bytes: &[u8]) {
        debug_assert!(
            self.is_byte_aligned(),
            "write_aligned_bytes needs byte alignment"
        );
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.bytes.push((self.cache >> self.nbits) as u8);
        }
        self.cache = 0; // nbits == 0 here; restore the "zero above nbits" invariant
        self.bytes.extend_from_slice(bytes);
    }

    /// Writes a single bit (`true` => 1).
    #[inline]
    pub fn write_bit(&mut self, bit: bool) {
        self.write_bits(bit as u32, 1);
    }

    /// Appends every bit of `other`, in order, at this writer's current position.
    ///
    /// Neither writer need be byte-aligned. This is what lets a macroblock be
    /// encoded into a scratch writer *before* the syntax that must precede it is
    /// known (`mb_skip_run` is only decided once the macroblock's own cost is), so
    /// the encoder can commit one encode rather than trialing and repeating it.
    /// Sound for CAVLC because a macroblock's Exp-Golomb/VLC syntax does not depend
    /// on its bit position; an arithmetic coder (CABAC) could NOT be spliced this
    /// way, since its state is carried across the whole slice.
    pub fn append(&mut self, other: &BitWriter) {
        for chunk in other.bytes.chunks(4) {
            let mut word = 0u32;
            for &b in chunk {
                word = (word << 8) | b as u32;
            }
            self.write_bits(word, chunk.len() as u32 * 8);
        }
        if other.nbits > 0 {
            // `cache` holds `nbits` (<= 31) valid bits in its low positions.
            self.write_bits(other.cache as u32, other.nbits);
        }
    }

    /// Emits a value already mapped to its Exp-Golomb code number: `floor(log2 x)`
    /// leading zeros then `x` in `floor(log2 x)+1` bits, where `x = code_num + 1`.
    #[inline]
    fn put_golomb(&mut self, x: u64) {
        let n = 63 - x.leading_zeros(); // floor(log2 x), 0..=32
        self.write_bits(0, n);
        if n < 32 {
            self.write_bits(x as u32, n + 1);
        } else {
            // n == 32 (e.g. ue(u32::MAX)): 33-bit value, split across two writes.
            self.write_bits((x >> 32) as u32, n - 31);
            self.write_bits(x as u32, 32);
        }
    }

    /// Unsigned Exp-Golomb code `ue(v)`.
    pub fn write_ue(&mut self, value: u32) {
        self.put_golomb(value as u64 + 1);
    }

    /// Signed Exp-Golomb code `se(v)`: `0->0, 1->1, -1->2, 2->3, -2->4, ...`.
    pub fn write_se(&mut self, value: i32) {
        let code_num = if value <= 0 {
            (-(value as i64) as u64) * 2
        } else {
            (value as u64) * 2 - 1
        };
        self.put_golomb(code_num + 1);
    }

    /// Writes the `rbsp_trailing_bits()`: a stop bit `1` then zero-pad to a byte.
    pub fn rbsp_trailing_bits(&mut self) {
        self.write_bits(1, 1);
        self.align_zero();
    }

    /// Pads with zero bits to the next byte boundary (no stop bit), then flushes
    /// the pending whole bytes from the cache (the word-flush in `write_bits` can
    /// leave up to 31 pending bits).
    pub fn align_zero(&mut self) {
        let pad = (8 - self.nbits % 8) % 8;
        if pad != 0 {
            self.write_bits(0, pad);
        }
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.bytes.push((self.cache >> self.nbits) as u8);
        }
        self.cache = 0;
    }

    /// Consumes the writer, returning the byte buffer. Flushes any whole bytes
    /// still pending in the cache; panics if a sub-byte remainder is left (call
    /// [`rbsp_trailing_bits`](Self::rbsp_trailing_bits) or
    /// [`align_zero`](Self::align_zero) first).
    pub fn into_bytes(mut self) -> Vec<u8> {
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.bytes.push((self.cache >> self.nbits) as u8);
        }
        assert!(
            self.nbits == 0,
            "BitWriter::into_bytes called with {} dangling bits",
            self.nbits
        );
        self.bytes
    }

    /// Borrows the completed bytes (excludes bits still pending in the cache).
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// [`into_bytes`](Self::into_bytes) without consuming the writer: flushes
    /// the whole bytes still pending and returns them borrowed, so a reused
    /// writer keeps its allocation across frames. Panics on a sub-byte
    /// remainder, like `into_bytes`.
    pub fn finish(&mut self) -> &[u8] {
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.bytes.push((self.cache >> self.nbits) as u8);
        }
        assert!(
            self.nbits == 0,
            "BitWriter::finish called with {} dangling bits",
            self.nbits
        );
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_bits_is_msb_first() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        w.write_bits(0b1, 1);
        w.align_zero();
        // bits 1,0,1,1 then zero-pad -> 1011_0000
        assert_eq!(w.into_bytes(), vec![0b1011_0000]);
    }

    #[test]
    fn ue_known_values() {
        // From H.264 Table 9-2: code_num -> bit string.
        let cases: &[(u32, &str)] = &[
            (0, "1"),
            (1, "010"),
            (2, "011"),
            (3, "00100"),
            (4, "00101"),
            (5, "00110"),
            (6, "00111"),
            (7, "0001000"),
            (8, "0001001"),
        ];
        for &(v, bits) in cases {
            let mut w = BitWriter::new();
            w.write_ue(v);
            assert_eq!(bitstring(&w), bits, "ue({v})");
        }
    }

    #[test]
    fn se_known_values() {
        // H.264 Table 9-3 mapping: se -> code_num.
        let cases: &[(i32, &str)] = &[
            (0, "1"),    // code_num 0
            (1, "010"),  // 1
            (-1, "011"), // 2
            (2, "00100"),
            (-2, "00101"),
            (3, "00110"),
            (-3, "00111"),
        ];
        for &(v, bits) in cases {
            let mut w = BitWriter::new();
            w.write_se(v);
            assert_eq!(bitstring(&w), bits, "se({v})");
        }
    }

    #[test]
    fn ue_max_does_not_overflow() {
        let mut w = BitWriter::new();
        w.write_ue(u32::MAX);
        // value+1 = 2^32 -> 32 leading zeros then 33-bit value => 65 bits total.
        assert_eq!(w.bit_len(), 65);
    }

    #[test]
    fn rbsp_trailing_aligns_to_byte() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        w.rbsp_trailing_bits();
        // 101 + stop 1 + pad 0000 => 1011_0000
        assert_eq!(w.into_bytes(), vec![0b1011_0000]);
    }

    /// Renders the bits currently buffered (including the partial byte) as a
    /// string of '0'/'1' for assertions.
    fn bitstring(w: &BitWriter) -> String {
        let mut s = String::new();
        for &b in w.as_bytes() {
            for i in (0..8).rev() {
                s.push(if (b >> i) & 1 == 1 { '1' } else { '0' });
            }
        }
        for i in (0..w.nbits).rev() {
            s.push(if (w.cache >> i) & 1 == 1 { '1' } else { '0' });
        }
        s
    }
}

#[cfg(test)]
mod append_tests {
    use super::BitWriter;

    /// Appending must be indistinguishable from having written the bits inline,
    /// at every source/destination bit misalignment — this is the invariant the
    /// encoder's encode-once-and-splice path rests on.
    #[test]
    fn append_matches_inline_at_every_alignment() {
        for lead in 0..17u32 {
            for n in 0..40u32 {
                let mut inline = BitWriter::new();
                inline.write_bits(0b1011_0110, lead.min(8));
                if lead > 8 {
                    inline.write_bits(0x5A5A, lead - 8);
                }
                let mut spliced = inline.clone();

                // the payload, written bit-patterned so order errors show up
                let mut scratch = BitWriter::new();
                for i in 0..n {
                    scratch.write_bit(i % 3 == 0);
                }
                for i in 0..n {
                    inline.write_bit(i % 3 == 0);
                }
                spliced.append(&scratch);

                assert_eq!(
                    spliced.bit_len(),
                    inline.bit_len(),
                    "lead={lead} n={n}: length"
                );
                let (mut a, mut b) = (spliced.clone(), inline.clone());
                a.align_zero();
                b.align_zero();
                assert_eq!(a.into_bytes(), b.into_bytes(), "lead={lead} n={n}: bits");
            }
        }
    }

    /// Exp-Golomb payloads across a byte-crossing boundary (the real shape).
    #[test]
    fn append_preserves_exp_golomb() {
        for lead in 0..9u32 {
            let mut inline = BitWriter::new();
            inline.write_bits(1, lead);
            let mut spliced = inline.clone();
            let mut scratch = BitWriter::new();
            for v in [0u32, 1, 7, 40, 1000, 65535] {
                scratch.write_ue(v);
                inline.write_ue(v);
            }
            spliced.append(&scratch);
            assert_eq!(spliced.bit_len(), inline.bit_len(), "lead={lead}: length");
            spliced.align_zero();
            inline.align_zero();
            assert_eq!(spliced.into_bytes(), inline.into_bytes(), "lead={lead}");
        }
    }
}
