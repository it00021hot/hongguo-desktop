//! MSB-first bit reader with H.264 Exp-Golomb decoding.
//!
//! The inverse of [`crate::BitWriter`]. Operates over an RBSP byte slice
//! (emulation-prevention bytes already removed — see [`crate::nal`]).

#[allow(unused_imports)]
use alloc::vec::Vec;

/// Error returned when a read runs past the end of the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfData;

impl core::fmt::Display for OutOfData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("bit reader ran out of data")
    }
}

impl core::error::Error for OutOfData {}

/// A big-endian, MSB-first bit reader.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Absolute bit position from the start of `data`.
    pos: usize,
    /// Bit position of the RBSP stop bit — a CONSTANT of the buffer, computed
    /// once here. `more_rbsp_data` used to rediscover it with a reverse byte
    /// scan on EVERY call (twice per CAVLC macroblock).
    stop_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Wraps an RBSP byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        let stop_pos = data
            .iter()
            .enumerate()
            .rev()
            .find(|(_, &b)| b != 0)
            .map_or(0, |(bi, &b)| bi * 8 + (7 - b.trailing_zeros() as usize));
        Self {
            data,
            pos: 0,
            stop_pos,
        }
    }

    /// The underlying RBSP buffer (for handing off to the CABAC engine).
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Current bit position.
    pub fn bit_pos(&self) -> usize {
        self.pos
    }

    /// Total number of bits in the buffer.
    pub fn bit_len(&self) -> usize {
        self.data.len() * 8
    }

    /// `more_rbsp_data()` (spec §7.2): true while the read position is before the
    /// `rbsp_stop_one_bit` (the last set bit in the buffer). Used to detect the
    /// end of `slice_data()` when a picture is split into multiple slices.
    #[inline]
    pub fn more_rbsp_data(&self) -> bool {
        // Stop bit = the last 1 bit in the buffer (cached at construction; an
        // all-zero buffer caches 0, and pos < 0 is false — same as the old
        // None arm).
        self.pos < self.stop_pos
    }

    /// `true` if the read position sits on a byte boundary.
    pub fn is_byte_aligned(&self) -> bool {
        self.pos % 8 == 0
    }

    /// Advances to the next byte boundary, consuming the intervening bits (e.g.
    /// `pcm_alignment_zero_bit`s before an `I_PCM` payload).
    pub fn align_to_byte(&mut self) -> Result<(), OutOfData> {
        while self.pos % 8 != 0 {
            self.read_bit()?;
        }
        Ok(())
    }

    /// Reads a single bit.
    pub fn read_bit(&mut self) -> Result<bool, OutOfData> {
        // `.get` rather than a `bit_len()` guard plus an index. `bit_len()` is
        // `data.len() * 8`, so the guard DOES imply the index is in range — but
        // the multiply can overflow in principle, which is exactly why LLVM will
        // not fold the check. Indexing fallibly says the same thing in a form it
        // can use, on the hottest read in the decoder.
        let Some(&byte) = self.data.get(self.pos / 8) else {
            return Err(OutOfData);
        };
        let bit = (byte >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Ok(bit == 1)
    }

    /// Reads `n` bits (`n` <= 32) as an unsigned value, MSB first. `u(n)`.
    pub fn read_bits(&mut self, n: u32) -> Result<u32, OutOfData> {
        // More than 32 bits cannot fit a u32. Rather than panic on a hostile
        // length (e.g. a corrupt log2_* field driving the count), reject it.
        if n > 32 {
            return Err(OutOfData);
        }
        if n == 0 {
            return Ok(0);
        }
        if n <= 24 {
            let v = self.peek_bits(n);
            self.skip_bits(n)?;
            return Ok(v);
        }
        // n in 25..=32: two chunks (peek_bits caps at 24).
        let hi = self.read_bits(n - 16)?;
        let lo = self.read_bits(16)?;
        Ok((hi << 16) | lo)
    }

    /// Peeks the next `n` bits (`n` ≤ 24) as an MSB-first value **without
    /// consuming**, zero-filling past the end of the buffer. O(1): loads up to 4
    /// bytes. The zero-fill lets a VLC/Exp-Golomb table match be attempted at the
    /// stream end; the caller then [`skip_bits`](Self::skip_bits)s the matched
    /// length, which rejects (OutOfData) if those bits ran past the buffer.
    #[inline]
    pub fn peek_bits(&self, n: u32) -> u32 {
        debug_assert!(n <= 24);
        let byte = self.pos / 8;
        let off = (self.pos % 8) as u32;
        // 4 bytes (zero past end), MSB-first, into a 32-bit window.
        //
        // FAST PATH: one RANGE check and one 4-byte big-endian load, which LLVM
        // lowers to `mov` + `bswap`. The previous form did FOUR separate
        // bounds-checked `get().unwrap_or(&0)` loads plus three shifts and three ORs,
        // on a function the entropy decoder calls tens of millions of times per
        // sequence. No `unsafe` needed — `get(range)` + `from_be_bytes` is safe and
        // compiles to the same thing an unchecked load would.
        //
        // The slow arm keeps the EXACT zero-fill-past-the-end contract the doc
        // comment promises (and that VLC matching at stream end relies on), so the
        // two arms are indistinguishable to every caller; `peek_bits_tail_matches`
        // pins that across the whole boundary region.
        let acc = match self.data.get(byte..byte + 4) {
            Some(c) => u32::from_be_bytes([c[0], c[1], c[2], c[3]]),
            None => {
                ((*self.data.get(byte).unwrap_or(&0) as u32) << 24)
                    | ((*self.data.get(byte + 1).unwrap_or(&0) as u32) << 16)
                    | ((*self.data.get(byte + 2).unwrap_or(&0) as u32) << 8)
                    | (*self.data.get(byte + 3).unwrap_or(&0) as u32)
            }
        };
        // The bit at `pos` is window bit (31 − off); take the `n` bits below it.
        (acc >> (32 - off - n)) & ((1u32 << n) - 1)
    }

    /// Consumes `n` bits (advances the position), after a [`peek_bits`]. Rejects
    /// if the bits run past the end of the buffer — the truncated-stream guard
    /// that `read_bit`'s per-bit bounds check provided.
    #[inline]
    pub fn skip_bits(&mut self, n: u32) -> Result<(), OutOfData> {
        if self.pos + n as usize > self.bit_len() {
            return Err(OutOfData);
        }
        self.pos += n as usize;
        Ok(())
    }

    /// Unsigned Exp-Golomb decode, `ue(v)`.
    pub fn read_ue(&mut self) -> Result<u32, OutOfData> {
        // Fast path: a codeword with `lz` leading zeros is `2·lz+1` bits. With
        // `lz ≤ 11` the whole codeword fits the 24-bit peek window — find `lz`
        // by counting leading zeros, then extract value in one shot.
        let window = self.peek_bits(24);
        let lz = window.leading_zeros() - 8; // leading zeros within the 24-bit window
        if lz <= 11 {
            let total = 2 * lz + 1;
            self.skip_bits(total)?;
            if lz == 0 {
                return Ok(0);
            }
            let info = (window >> (24 - total)) & ((1u32 << lz) - 1);
            return Ok((1u32 << lz) - 1 + info);
        }
        // ≥12 leading zeros (huge value or run of zeros): exact bit-at-a-time.
        let mut leading_zeros = 0u32;
        while !self.read_bit()? {
            leading_zeros += 1;
            // 32 leading zeros would make `1 << leading_zeros` overflow u32 (and
            // the value is not representable anyway) — reject as malformed.
            if leading_zeros >= 32 {
                return Err(OutOfData);
            }
        }
        if leading_zeros == 0 {
            return Ok(0);
        }
        let info = self.read_bits(leading_zeros)?;
        Ok((1u32 << leading_zeros) - 1 + info)
    }

    /// Signed Exp-Golomb decode, `se(v)`.
    pub fn read_se(&mut self) -> Result<i32, OutOfData> {
        let code_num = self.read_ue()?;
        // Inverse of the se->code_num mapping.
        let magnitude = code_num.div_ceil(2) as i32;
        Ok(if code_num % 2 == 1 {
            magnitude
        } else {
            -magnitude
        })
    }
}

/// A BY-VALUE read cursor over a [`BitReader`]'s buffer. The CAVLC residual
/// decoder pulls one at block entry, decodes every symbol of the block against
/// it, and commits the position back once. Through `&mut BitReader` the same
/// work reloaded `(data.ptr, data.len, pos)` before every peek and stored `pos`
/// after every skip — four times per symbol on the hottest path of the CAVLC
/// tier — because the out-of-line calls in the old body forced the state back
/// to memory. Byte-for-byte the same reads; the position is committed on `Ok`.
#[derive(Clone, Copy, Debug)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    /// Snapshot the read position into a register-resident cursor.
    #[inline(always)]
    pub fn cursor(&self) -> Cursor<'a> {
        Cursor { data: self.data, pos: self.pos }
    }

    /// Adopt a cursor's position. The cursor must have come from `self.cursor()`.
    #[inline(always)]
    pub fn commit(&mut self, c: Cursor<'a>) {
        debug_assert!(core::ptr::eq(self.data, c.data));
        self.pos = c.pos;
    }
}

/// The past-the-end zero-fill arm of the 4-byte window load: cold by
/// construction (it fires only within the last three bytes of a slice), so it
/// lives out of line instead of being inlined ~30 instructions at a time into
/// every peek site of the residual decoder.
#[cold]
#[inline(never)]
fn window_tail(data: &[u8], byte: usize) -> u32 {
    ((*data.get(byte).unwrap_or(&0) as u32) << 24)
        | ((*data.get(byte + 1).unwrap_or(&0) as u32) << 16)
        | ((*data.get(byte + 2).unwrap_or(&0) as u32) << 8)
        | (*data.get(byte + 3).unwrap_or(&0) as u32)
}

impl<'a> Cursor<'a> {
    /// Rebuild a cursor from its two words. Used to hand the cursor across a
    /// non-inlined call as SCALARS: an aggregate argument is passed by hidden
    /// pointer on x86-64 Windows and rustc then homes the local IN that memory,
    /// so the callee would keep `pos` in a load/store pair per symbol.
    #[inline(always)]
    pub fn from_parts(data: &'a [u8], pos: usize) -> Self {
        Cursor { data, pos }
    }

    /// The two words of the cursor.
    #[inline(always)]
    pub fn into_parts(self) -> (&'a [u8], usize) {
        (self.data, self.pos)
    }

    /// Current absolute bit position.
    #[inline(always)]
    pub fn bit_pos(&self) -> usize {
        self.pos
    }

    /// The next 24 bits, MSB-first, zero-filled past the end of the buffer
    /// (exactly [`BitReader::peek_bits`]`(24)`). One range check and one
    /// big-endian load on the fast arm.
    #[inline(always)]
    pub fn peek24(&self) -> u32 {
        let byte = self.pos >> 3;
        let off = (self.pos & 7) as u32;
        let acc = match self.data.get(byte..byte + 4) {
            Some(c) => u32::from_be_bytes([c[0], c[1], c[2], c[3]]),
            None => window_tail(self.data, byte),
        };
        (acc >> (8 - off)) & 0x00FF_FFFF
    }

    /// The next `n` bits (`n` ≤ 24) without consuming — `peek24 >> (24 - n)`.
    #[inline(always)]
    pub fn peek(&self, n: u32) -> u32 {
        debug_assert!(n <= 24);
        self.peek24() >> (24 - n)
    }

    /// Consume `n` bits; rejects a run past the end of the buffer.
    #[inline(always)]
    pub fn skip(&mut self, n: u32) -> Result<(), OutOfData> {
        let np = self.pos + n as usize;
        if np > self.data.len() * 8 {
            return Err(OutOfData);
        }
        self.pos = np;
        Ok(())
    }

    /// One bit, consuming. Fallible index (see [`BitReader::read_bit`]).
    #[inline(always)]
    pub fn read_bit(&mut self) -> Result<bool, OutOfData> {
        let Some(&byte) = self.data.get(self.pos >> 3) else {
            return Err(OutOfData);
        };
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Ok(bit == 1)
    }

    /// `u(n)` for `n` ≤ 32. The `n ≤ 24` arm is one peek + one skip, inline;
    /// wider reads (a CAVLC level_prefix ≥ 28, never seen on a conformant
    /// stream) take the cold two-chunk arm.
    #[inline(always)]
    pub fn read_bits(&mut self, n: u32) -> Result<u32, OutOfData> {
        if n <= 24 {
            let v = self.peek(n);
            self.skip(n)?;
            return Ok(v);
        }
        // BY VALUE in and out: a `&mut self` here would make the cursor escape to
        // a call, forcing every caller to keep it in memory across the hot loop.
        let (v, c) = self.read_bits_long(n)?;
        *self = c;
        Ok(v)
    }

    #[cold]
    #[inline(never)]
    fn read_bits_long(mut self, n: u32) -> Result<(u32, Self), OutOfData> {
        if n > 32 {
            return Err(OutOfData);
        }
        let hi = self.read_bits(n - 16)?;
        let lo = self.read_bits(16)?;
        Ok(((hi << 16) | lo, self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BitWriter;

    /// The one-load fast path must equal the byte-at-a-time zero-fill reference at
    /// EVERY bit position and width — including the last bytes, where the fast arm
    /// stops applying and the tail arm takes over. A mismatch there would corrupt
    /// VLC matching only at stream end, which is exactly the bug a mid-buffer test
    /// would miss.
    #[test]
    fn peek_bits_matches_zero_fill_reference() {
        fn reference(data: &[u8], pos: usize, n: u32) -> u32 {
            let byte = pos / 8;
            let off = (pos % 8) as u32;
            let acc = ((*data.get(byte).unwrap_or(&0) as u32) << 24)
                | ((*data.get(byte + 1).unwrap_or(&0) as u32) << 16)
                | ((*data.get(byte + 2).unwrap_or(&0) as u32) << 8)
                | (*data.get(byte + 3).unwrap_or(&0) as u32);
            (acc >> (32 - off - n)) & ((1u32 << n) - 1)
        }
        let mut st = 0x1234_5678u32;
        let mut rnd = || {
            st ^= st << 13;
            st ^= st >> 17;
            st ^= st << 5;
            st
        };
        for len in 1..24usize {
            let data: Vec<u8> = (0..len).map(|_| (rnd() >> 7) as u8).collect();
            let r = BitReader::new(&data);
            for pos in 0..len * 8 {
                for n in 1..=24u32 {
                    let mut br = BitReader::new(&data);
                    br.skip_bits(pos as u32).ok();
                    // skip_bits refuses past the end; drive `pos` directly instead.
                    let mut probe = BitReader::new(&data);
                    while probe.bit_pos() < pos {
                        if probe.read_bit().is_err() {
                            break;
                        }
                    }
                    if probe.bit_pos() != pos {
                        continue;
                    }
                    assert_eq!(
                        probe.peek_bits(n),
                        reference(&data, pos, n),
                        "len={len} pos={pos} n={n}"
                    );
                }
            }
            let _ = r;
        }
    }

    #[test]
    fn roundtrip_ue() {
        for v in [0u32, 1, 2, 3, 4, 7, 8, 255, 256, 65535, u32::MAX - 1] {
            let mut w = BitWriter::new();
            w.write_ue(v);
            w.align_zero();
            let bytes = w.into_bytes();
            let mut r = BitReader::new(&bytes);
            assert_eq!(r.read_ue().unwrap(), v, "ue roundtrip {v}");
        }
    }

    #[test]
    fn roundtrip_se() {
        for v in [0i32, 1, -1, 2, -2, 100, -100, 32767, -32768] {
            let mut w = BitWriter::new();
            w.write_se(v);
            w.align_zero();
            let bytes = w.into_bytes();
            let mut r = BitReader::new(&bytes);
            assert_eq!(r.read_se().unwrap(), v, "se roundtrip {v}");
        }
    }

    #[test]
    fn roundtrip_mixed_stream() {
        let mut w = BitWriter::new();
        w.write_bits(0b1011, 4);
        w.write_ue(42);
        w.write_se(-17);
        w.write_bits(1, 1);
        w.align_zero();
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_bits(4).unwrap(), 0b1011);
        assert_eq!(r.read_ue().unwrap(), 42);
        assert_eq!(r.read_se().unwrap(), -17);
        assert_eq!(r.read_bits(1).unwrap(), 1);
    }

    #[test]
    fn reports_out_of_data() {
        let bytes = [0x80u8];
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_bits(8).unwrap(), 0x80);
        assert_eq!(r.read_bit(), Err(OutOfData));
    }

    #[test]
    fn peek_then_skip_matches_read_bits() {
        let bytes = [0xB5u8, 0x3C, 0xF0, 0x0A, 0x77];
        // At every bit offset and width, peek_bits + skip_bits must equal a
        // consuming read_bits from a fresh reader at the same position.
        for start in 0..16u32 {
            for n in 1..=24u32 {
                let mut a = BitReader::new(&bytes);
                a.skip_bits(start).unwrap();
                let peeked = a.peek_bits(n);
                let pos_before = a.bit_pos();
                a.skip_bits(n).unwrap();
                assert_eq!(a.bit_pos(), pos_before + n as usize);

                let mut b = BitReader::new(&bytes);
                b.skip_bits(start).unwrap();
                assert_eq!(peeked, b.read_bits(n).unwrap(), "start={start} n={n}");
            }
        }
    }

    #[test]
    fn peek_zero_fills_past_end() {
        let bytes = [0xFFu8];
        let mut r = BitReader::new(&bytes);
        r.skip_bits(4).unwrap();
        // 4 real bits (1111) then zero-fill: 0b1111_0000_0000... for 12 bits.
        assert_eq!(r.peek_bits(12), 0b1111_0000_0000);
        // skipping past the end is rejected.
        assert_eq!(r.skip_bits(8), Err(OutOfData));
    }
}
