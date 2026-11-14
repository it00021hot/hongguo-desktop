mod annexb;
mod metadata;
mod parameters;
use annexb::split;
#[cfg(test)]
mod tests;

use hongguo_media_types::{ColorMetadata, CropRect, FrameDescriptor};
use rusty_h265::{
    Error, Result,
    bits::BitReader,
    nal::{NalHeader, NalType, unescape},
    ps::{Pps, Sps, parse_pps, parse_sps},
    slice::{SliceHeader, parse_slice_header},
};
use std::{collections::BTreeMap, sync::Arc};

pub const MAX_ACCESS_UNIT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SLICES: usize = 600;
pub const MAX_REFERENCES: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    Before,
    After,
    LongTerm,
    Following,
    LongTermFollowing,
}
#[derive(Clone, Copy, Debug)]
pub struct Reference {
    pub poc: i32,
    pub lsb_mask: Option<i32>,
    pub kind: ReferenceKind,
}
impl Reference {
    pub fn matches(&self, poc: i32) -> bool {
        self.lsb_mask
            .map_or(poc == self.poc, |mask| poc & mask == self.poc & mask)
    }
    pub fn long_term(&self) -> bool {
        matches!(
            self.kind,
            ReferenceKind::LongTerm | ReferenceKind::LongTermFollowing
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReferenceSurface {
    pub poc: i32,
    pub slot: u8,
    pub long_term: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Slice {
    pub offset: u32,
    pub length: u32,
}

pub struct Picture {
    pub descriptor: FrameDescriptor,
    pub poc: i32,
    pub references: Vec<Reference>,
    pub bitstream: Vec<u8>,
    pub slices: Vec<Slice>,
    pub output: bool,
    pub reference: bool,
    pub reset: bool,
    pub discard_prior: bool,
    pub skip: bool,
    pub reorder: usize,
    pub dpb_size: usize,
    sps: Arc<Sps>,
    pps: Arc<Pps>,
    header: SliceHeader,
    nal: NalHeader,
    reference_rps_delta_pocs: u8,
}

pub struct Parser {
    sps: BTreeMap<u8, Arc<Sps>>,
    pps: BTreeMap<u8, Arc<Pps>>,
    colors: BTreeMap<u8, ColorMetadata>,
    hdr: ColorMetadata,
    previous_poc: i32,
    first: bool,
    no_rasl: bool,
    active_sps_id: Option<u8>,
}

impl Default for Parser {
    fn default() -> Self {
        Self {
            sps: BTreeMap::new(),
            pps: BTreeMap::new(),
            colors: BTreeMap::new(),
            hdr: ColorMetadata::default(),
            previous_poc: 0,
            first: true,
            no_rasl: false,
            active_sps_id: None,
        }
    }
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn color(&mut self, annex_b: &[u8]) -> Result<ColorMetadata> {
        self.parameters(annex_b)?;
        let id = self
            .active_sps_id
            .or_else(|| self.sps.keys().next().copied())
            .ok_or_else(|| Error::invalid("missing HEVC SPS"))?;
        let mut color = self
            .colors
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::invalid("missing active HEVC color metadata"))?;
        color.mastering_display = self.hdr.mastering_display;
        color.content_light = self.hdr.content_light;
        Ok(color)
    }

    pub fn parameters(&mut self, annex_b: &[u8]) -> Result<()> {
        if annex_b.is_empty() {
            return Ok(());
        }
        for bytes in split(annex_b)? {
            self.parameter(bytes)?;
        }
        Ok(())
    }

    fn parameter(&mut self, bytes: &[u8]) -> Result<()> {
        let nal =
            NalHeader::parse(bytes).ok_or_else(|| Error::invalid("invalid HEVC NAL header"))?;
        if nal.layer_id != 0 {
            return Err(Error::unsupported("multilayer HEVC hardware decoding"));
        }
        if nal.nal_type.is_vcl() {
            // PPS selection occupies at most 15 RBSP bits before the rest of the slice header.
            let rbsp = unescape(&bytes[2..bytes.len().min(18)]);
            let mut reader = BitReader::new(&rbsp.data);
            reader.read_flag()?;
            if nal.nal_type.is_irap() {
                reader.read_flag()?;
            }
            let id = reader.read_ue_max(63)? as u8;
            self.active_sps_id = Some(
                self.pps
                    .get(&id)
                    .ok_or_else(|| Error::invalid("missing active HEVC PPS"))?
                    .sps_id,
            );
            return Ok(());
        }
        let rbsp = unescape(&bytes[2..]);
        match nal.nal_type {
            NalType::Sps => {
                let sps = parse_sps(&rbsp.data)?;
                self.colors.insert(sps.id, metadata::sps(&rbsp.data, &sps)?);
                self.sps.insert(sps.id, Arc::new(sps));
            }
            NalType::Pps => {
                let pps = parse_pps(&rbsp.data)?;
                self.pps.insert(pps.id, Arc::new(pps));
            }
            NalType::PrefixSei | NalType::SuffixSei => metadata::sei(&rbsp.data, &mut self.hdr)?,
            _ => {}
        }
        Ok(())
    }

    pub fn format(&self) -> Result<FrameDescriptor> {
        let sps = self.active_sps()?;
        self.descriptor(sps)
    }
    pub fn dpb_size(&self) -> Result<usize> {
        let sps = self.active_sps()?;
        Ok(
            sps.ordering[sps.max_sub_layers_minus1 as usize].max_dec_pic_buffering_minus1 as usize
                + 1,
        )
    }

    fn active_sps(&self) -> Result<&Sps> {
        self.active_sps_id
            .and_then(|id| self.sps.get(&id))
            .or_else(|| self.sps.values().next())
            .map(Arc::as_ref)
            .ok_or_else(|| Error::invalid("missing HEVC SPS"))
    }

    fn descriptor(&self, sps: &Sps) -> Result<FrameDescriptor> {
        if !matches!(sps.ptl.profile(), 1..=3)
            || sps.chroma_format_idc != 1
            || !matches!(sps.bit_depth_luma, 8 | 10)
            || sps.bit_depth_luma != sps.bit_depth_chroma
            || sps.range_extension
            || sps.multilayer_extension
            || sps.ext_3d
            || sps.scc_extension
            || sps.ptl.interlaced_source_flag
        {
            return Err(Error::unsupported(
                "D3D11 HEVC requires progressive Main/Main10 4:2:0",
            ));
        }
        let (width, height) = sps.output_size();
        if width == 0 || height == 0 {
            return Err(Error::invalid("empty HEVC conformance window"));
        }
        let mut color = self.colors.get(&sps.id).cloned().unwrap_or_default();
        color.mastering_display = self.hdr.mastering_display;
        color.content_light = self.hdr.content_light;
        Ok(FrameDescriptor {
            width: sps.width as usize,
            height: sps.height as usize,
            bit_depth: sps.bit_depth_luma,
            crop: CropRect {
                x: sps.conf_win[0] as usize * 2,
                y: sps.conf_win[2] as usize * 2,
                width: width as usize,
                height: height as usize,
            },
            rotation_degrees: 0,
            color,
        })
    }

    pub fn picture(&mut self, annex_b: &[u8]) -> Result<Option<Picture>> {
        if annex_b.is_empty() || annex_b.len() > MAX_ACCESS_UNIT_BYTES {
            return Err(Error::invalid("empty or oversized HEVC access unit"));
        }
        let mut first: Option<(NalHeader, SliceHeader)> = None;
        let mut bitstream = Vec::new();
        let mut slices: Vec<Slice> = Vec::new();
        let mut reference_rps_delta_pocs = 0;
        let mut previous_independent = None;
        for bytes in split(annex_b)? {
            let nal =
                NalHeader::parse(bytes).ok_or_else(|| Error::invalid("invalid HEVC NAL header"))?;
            if !nal.nal_type.is_vcl() {
                self.parameter(bytes)?;
                continue;
            }
            if nal.layer_id != 0 {
                return Err(Error::unsupported("multilayer HEVC picture"));
            }
            let rbsp = unescape(&bytes[2..]);
            let header = parse_slice_header(
                &mut BitReader::new(&rbsp.data),
                &nal,
                &|id| {
                    let pps = self.pps.get(&id)?;
                    Some((self.sps.get(&pps.sps_id)?.as_ref(), pps.as_ref()))
                },
                previous_independent.as_ref(),
            )?;
            let dependent = header.dependent_slice_segment;
            if !header.dependent_slice_segment {
                previous_independent = Some(header.clone());
            }
            if let Some((old_nal, old)) = &first {
                if header.first_slice_segment_in_pic
                    || header.pps_id != old.pps_id
                    || header.poc_lsb != old.poc_lsb
                    || nal.nal_type != old_nal.nal_type
                    || header.st_rps != old.st_rps
                    || header.long_term != old.long_term
                    || header.pic_output_flag != old.pic_output_flag
                    || nal.temporal_id != old_nal.temporal_id
                {
                    return Err(Error::invalid("multiple HEVC pictures in one access unit"));
                }
            } else {
                if !header.first_slice_segment_in_pic {
                    return Err(Error::invalid(
                        "HEVC access unit starts with a continuation slice",
                    ));
                }
                let pps = self
                    .pps
                    .get(&header.pps_id)
                    .ok_or_else(|| Error::invalid("missing active PPS"))?;
                let sps = self
                    .sps
                    .get(&pps.sps_id)
                    .ok_or_else(|| Error::invalid("missing active SPS"))?;
                reference_rps_delta_pocs =
                    parameters::reference_rps_count(&rbsp.data, sps, pps, nal)?;
                first = Some((nal, header));
            }
            if !dependent && slices.len() == MAX_SLICES {
                return Err(Error::unsupported(
                    "HEVC slice count exceeds hardware budget",
                ));
            }
            let offset = bitstream.len() as u32;
            bitstream.extend_from_slice(&[0, 0, 1]);
            bitstream.extend_from_slice(bytes);
            if dependent {
                let slice = slices
                    .last_mut()
                    .ok_or_else(|| Error::invalid("dependent segment has no independent slice"))?;
                slice.length = bitstream.len() as u32 - slice.offset;
            } else {
                slices.push(Slice {
                    offset,
                    length: bitstream.len() as u32 - offset,
                });
            }
        }
        let Some((nal, header)) = first else {
            return Ok(None);
        };
        let pps = self
            .pps
            .get(&header.pps_id)
            .cloned()
            .ok_or_else(|| Error::invalid("missing active PPS"))?;
        let sps = self
            .sps
            .get(&pps.sps_id)
            .cloned()
            .ok_or_else(|| Error::invalid("missing active SPS"))?;
        if !pps.is_v1_only()
            || pps.range_extension
            || pps.num_tile_columns > 20
            || pps.num_tile_rows > 22
        {
            return Err(Error::unsupported(
                "HEVC PPS exceeds Main/Main10 DXVA scope",
            ));
        }
        let descriptor = self.descriptor(&sps)?;
        let kind = nal.nal_type;
        if kind.is_irap() {
            self.no_rasl = self.first || kind.is_idr() || kind.is_bla();
        }
        if self.first && !kind.is_irap() {
            return Err(Error::invalid(
                "HEVC hardware decoder requires a random-access picture",
            ));
        }
        let poc = self.poc(&sps, &header, nal)?;
        let references = references(&sps, &header, poc)?;
        if references.len() > MAX_REFERENCES {
            return Err(Error::unsupported(
                "HEVC reference set exceeds DXVA capacity",
            ));
        }
        self.first = false;
        let ordering = sps.ordering[sps.max_sub_layers_minus1 as usize];
        if ordering.max_dec_pic_buffering_minus1 >= 16 {
            return Err(Error::unsupported("HEVC DPB exceeds hardware budget"));
        }
        Ok(Some(Picture {
            descriptor,
            poc,
            references,
            bitstream,
            slices,
            output: header.pic_output_flag && !(self.no_rasl && kind.is_rasl()),
            reference: !kind.is_sub_layer_non_ref(),
            reset: kind.is_idr() || kind.is_bla(),
            discard_prior: header.no_output_of_prior_pics,
            skip: self.no_rasl && kind.is_rasl(),
            reorder: ordering.max_num_reorder_pics as usize,
            dpb_size: ordering.max_dec_pic_buffering_minus1 as usize + 1,
            sps,
            pps,
            header,
            nal,
            reference_rps_delta_pocs,
        }))
    }

    fn poc(&mut self, sps: &Sps, header: &SliceHeader, nal: NalHeader) -> Result<i32> {
        let maximum = 1_i64 << sps.log2_max_poc_lsb;
        let lsb = i64::from(header.poc_lsb);
        let previous_poc = i64::from(self.previous_poc);
        let previous = previous_poc & (maximum - 1);
        let msb = if nal.nal_type.is_irap() && self.no_rasl {
            0
        } else if lsb < previous && previous - lsb >= maximum / 2 {
            previous_poc - previous + maximum
        } else if lsb > previous && lsb - previous > maximum / 2 {
            previous_poc - previous - maximum
        } else {
            previous_poc - previous
        };
        let poc = i32::try_from(msb + lsb)
            .map_err(|_| Error::invalid("HEVC picture order count overflow"))?;
        if nal.temporal_id == 0
            && !nal.nal_type.is_rasl()
            && !nal.nal_type.is_radl()
            && !nal.nal_type.is_sub_layer_non_ref()
        {
            self.previous_poc = poc;
        }
        Ok(poc)
    }
}

fn references(sps: &Sps, header: &SliceHeader, poc: i32) -> Result<Vec<Reference>> {
    let mut references = Vec::new();
    if header.poc_lsb == 0 && header.st_rps.num_delta_pocs() == 0 && header.long_term.is_empty() {
        return Ok(references);
    }
    for (entries, kind) in [
        (&header.st_rps.neg, ReferenceKind::Before),
        (&header.st_rps.pos, ReferenceKind::After),
    ] {
        for &(delta, used) in entries {
            references.push(Reference {
                poc: poc
                    .checked_add(delta)
                    .ok_or_else(|| Error::invalid("HEVC reference POC overflow"))?,
                lsb_mask: None,
                kind: if used { kind } else { ReferenceKind::Following },
            });
        }
    }
    let maximum = 1_i32 << sps.log2_max_poc_lsb;
    for entry in &header.long_term {
        let full = i64::from(entry.poc_lsb) + i64::from(poc)
            - i64::from(entry.delta_poc_msb_cycle) * i64::from(maximum)
            - i64::from(poc & (maximum - 1));
        references.push(Reference {
            poc: if entry.delta_poc_msb_present {
                i32::try_from(full).map_err(|_| Error::invalid("HEVC long-term POC overflow"))?
            } else {
                entry.poc_lsb as i32
            },
            lsb_mask: (!entry.delta_poc_msb_present).then_some(maximum - 1),
            kind: if entry.used_by_curr_pic {
                ReferenceKind::LongTerm
            } else {
                ReferenceKind::LongTermFollowing
            },
        });
    }
    Ok(references)
}
