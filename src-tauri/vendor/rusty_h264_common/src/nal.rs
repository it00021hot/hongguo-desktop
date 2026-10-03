//! NAL units, Annex-B framing, and RBSP emulation-prevention.
//!
//! H.264 carries syntax in NAL (Network Abstraction Layer) units. In the
//! Annex-B byte stream each NAL is prefixed by a start code (`00 00 01`, often
//! `00 00 00 01`). Within a NAL, the raw payload (RBSP) is escaped so the
//! start-code pattern can never appear by accident: any `00 00 00/01/02/03`
//! gets a `03` emulation-prevention byte inserted, producing the SODB/EBSP.

#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

/// NAL unit type (`nal_unit_type`, 5 bits). Only the subset relevant to a
/// Constrained Baseline encoder/decoder is named; others are `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalUnitType {
    /// Coded slice of a non-IDR picture.
    NonIdrSlice,
    /// Coded slice of an IDR picture.
    IdrSlice,
    /// Supplemental enhancement information.
    Sei,
    /// Sequence parameter set.
    Sps,
    /// Picture parameter set.
    Pps,
    /// Access unit delimiter.
    AccessUnitDelimiter,
    /// Any type not specifically handled.
    Other(u8),
}

impl NalUnitType {
    /// Numeric `nal_unit_type` value.
    pub fn id(self) -> u8 {
        match self {
            NalUnitType::NonIdrSlice => 1,
            NalUnitType::IdrSlice => 5,
            NalUnitType::Sei => 6,
            NalUnitType::Sps => 7,
            NalUnitType::Pps => 8,
            NalUnitType::AccessUnitDelimiter => 9,
            NalUnitType::Other(v) => v & 0x1f,
        }
    }

    /// Parses a `nal_unit_type` value.
    pub fn from_id(v: u8) -> Self {
        match v & 0x1f {
            1 => NalUnitType::NonIdrSlice,
            5 => NalUnitType::IdrSlice,
            6 => NalUnitType::Sei,
            7 => NalUnitType::Sps,
            8 => NalUnitType::Pps,
            9 => NalUnitType::AccessUnitDelimiter,
            other => NalUnitType::Other(other),
        }
    }
}

/// A NAL unit: a header (type + `nal_ref_idc`) plus its raw RBSP payload.
#[derive(Debug, Clone)]
pub struct NalUnit {
    /// `nal_ref_idc` (0..=3): non-zero marks the NAL as referenceable.
    pub ref_idc: u8,
    /// NAL unit type.
    pub nal_type: NalUnitType,
    /// Raw byte sequence payload (un-escaped).
    pub rbsp: Vec<u8>,
}

impl NalUnit {
    /// Builds a NAL unit.
    pub fn new(ref_idc: u8, nal_type: NalUnitType, rbsp: Vec<u8>) -> Self {
        Self {
            ref_idc,
            nal_type,
            rbsp,
        }
    }

    /// The one-byte NAL header: `forbidden_zero_bit(0) | nal_ref_idc | nal_unit_type`.
    pub fn header_byte(&self) -> u8 {
        ((self.ref_idc & 0x3) << 5) | self.nal_type.id()
    }

    /// Appends this NAL to `out` as an Annex-B unit: 4-byte start code, header
    /// byte, then the emulation-prevented payload.
    pub fn write_annex_b(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.push(self.header_byte());
        emulation_prevent_into(&self.rbsp, out);
    }

    /// Serializes a single NAL as a standalone Annex-B byte stream.
    pub fn to_annex_b(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rbsp.len() + 8);
        self.write_annex_b(&mut out);
        out
    }
}

/// Inserts emulation-prevention bytes, appending the EBSP to `out`.
///
/// Whenever the next byte would complete a `00 00 00`, `00 00 01`, `00 00 02`,
/// or `00 00 03` sequence, a `03` is inserted after the two zeros.
pub fn emulation_prevent_into(rbsp: &[u8], out: &mut Vec<u8>) {
    emulation_prevent_with(rbsp, |b| out.push(b));
}

/// The emulation-prevention scan behind [`emulation_prevent_into`], handing
/// each EBSP byte to `put` — so a caller-owned buffer can take the bytes
/// without an intermediate `Vec`. One scan for both sinks: they cannot drift.
pub fn emulation_prevent_with(rbsp: &[u8], mut put: impl FnMut(u8)) {
    let mut zeros = 0usize;
    for &b in rbsp {
        if zeros >= 2 && b <= 0x03 {
            put(0x03);
            zeros = 0;
        }
        put(b);
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
    }
}

/// The one-byte NAL header for `ref_idc` and `nal_type`.
pub fn nal_header_byte(ref_idc: u8, nal_type: NalUnitType) -> u8 {
    ((ref_idc & 0x3) << 5) | nal_type.id()
}

/// Removes emulation-prevention bytes from an EBSP payload, returning the RBSP.
///
/// D25 (inline-execution.md 11.11): most NALs carry NO emulation bytes, yet
/// this ran a fresh `Vec` + full copy per NAL. A scan-only pass (the SAME
/// predicate) now returns the input BORROWED when nothing needs dropping; the
/// original copy loop runs verbatim only when a 0x03 was actually found.
pub fn emulation_unprevent(ebsp: &[u8]) -> alloc::borrow::Cow<'_, [u8]> {
    let mut zeros = 0usize;
    let mut i = 0;
    let mut any = false;
    while i < ebsp.len() {
        let b = ebsp[i];
        if zeros >= 2 && b == 0x03 && i + 1 < ebsp.len() && ebsp[i + 1] <= 0x03 {
            any = true;
            break;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        i += 1;
    }
    if !any {
        return alloc::borrow::Cow::Borrowed(ebsp);
    }
    let mut out = Vec::with_capacity(ebsp.len());
    let mut zeros = 0usize;
    let mut i = 0;
    while i < ebsp.len() {
        let b = ebsp[i];
        if zeros >= 2 && b == 0x03 && i + 1 < ebsp.len() && ebsp[i + 1] <= 0x03 {
            // Drop this emulation-prevention byte.
            zeros = 0;
            i += 1;
            continue;
        }
        out.push(b);
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        i += 1;
    }
    alloc::borrow::Cow::Owned(out)
}

/// Splits an Annex-B byte stream into raw NAL byte slices (header + EBSP),
/// stripping start codes. Does not parse headers or un-escape payloads.
pub fn split_annex_b(stream: &[u8]) -> Vec<&[u8]> {
    // Single pass (11.11): the old form buffered every start position in a
    // second Vec and re-walked it. Closing the previous NAL as each start code
    // is FOUND emits the same slices with one Vec and one walk.
    let mut nals = Vec::new();
    let mut prev: Option<usize> = None;
    let mut i = 0;
    // `get(i..i + 3)` rather than a length test plus three whole-buffer indexes
    // (the same fix as `split_access_units`): this runs once per BYTE of stream.
    while let Some(w) = stream.get(i..i + 3) {
        if w[0] == 0 && w[1] == 0 && w[2] == 1 {
            if let Some(s) = prev {
                // Trim the trailing zero of a 4-byte (00 00 00 01) start code.
                let mut end = i;
                if end > s && stream.get(end - 1) == Some(&0) {
                    end -= 1;
                }
                if end > s {
                    nals.push(&stream[s..end]);
                }
            }
            prev = Some(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    if let Some(s) = prev {
        if stream.len() > s {
            nals.push(&stream[s..]);
        }
    }
    nals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_byte_packs_fields() {
        let nal = NalUnit::new(3, NalUnitType::Sps, vec![]);
        // ref_idc=3 (11), type=7 (00111) => 0110_0111
        assert_eq!(nal.header_byte(), 0b0110_0111);
    }

    #[test]
    fn emulation_prevention_inserts_03() {
        // Each inserted 0x03 resets the zero-run counter, so five zeros then a
        // 1 escapes twice: 00 00 [03] 00 00 [03] 00 01.
        let mut out = Vec::new();
        emulation_prevent_into(&[0, 0, 0, 0, 0, 1], &mut out);
        assert_eq!(out, vec![0, 0, 3, 0, 0, 3, 0, 1]);
    }

    #[test]
    fn emulation_roundtrip() {
        let payloads: &[&[u8]] = &[
            &[0, 0, 0, 1],
            &[0, 0, 1, 2, 3],
            &[1, 2, 3, 4, 5],
            &[0, 0, 0, 0, 0, 0],
            &[0, 0, 3, 0, 0, 3],
        ];
        for p in payloads {
            let mut ebsp = Vec::new();
            emulation_prevent_into(p, &mut ebsp);
            assert_eq!(&emulation_unprevent(&ebsp), p, "roundtrip {p:?}");
        }
    }

    #[test]
    fn annex_b_split_and_unescape() {
        let sps = NalUnit::new(3, NalUnitType::Sps, vec![0, 0, 1, 0x42]);
        let pps = NalUnit::new(3, NalUnitType::Pps, vec![0xAB, 0xCD]);
        let mut stream = Vec::new();
        sps.write_annex_b(&mut stream);
        pps.write_annex_b(&mut stream);

        let nals = split_annex_b(&stream);
        assert_eq!(nals.len(), 2);

        // First NAL: header byte then EBSP.
        assert_eq!(NalUnitType::from_id(nals[0][0]), NalUnitType::Sps);
        assert_eq!(emulation_unprevent(&nals[0][1..]), vec![0, 0, 1, 0x42]);

        assert_eq!(NalUnitType::from_id(nals[1][0]), NalUnitType::Pps);
        assert_eq!(emulation_unprevent(&nals[1][1..]), vec![0xAB, 0xCD]);
    }
}
