use super::*;

pub const PICTURE_PARAMETER_BYTES: usize = 232;
pub const QUANTIZATION_BYTES: usize = 1000;

impl Picture {
    pub fn parameters(
        &self,
        current: u8,
        surfaces: &[ReferenceSurface],
        feedback: u32,
    ) -> Result<Vec<u8>> {
        if current >= 128
            || surfaces.len() > MAX_REFERENCES
            || surfaces.iter().any(|surface| surface.slot >= 128)
        {
            return Err(Error::invalid("DXVA reference surface index"));
        }
        let s = &self.sps;
        for (index, surface) in surfaces.iter().enumerate() {
            if surface.slot == current
                || surfaces[..index]
                    .iter()
                    .any(|previous| previous.slot == surface.slot || previous.poc == surface.poc)
            {
                return Err(Error::invalid(
                    "duplicate or current DXVA reference surface",
                ));
            }
        }
        let p = &self.pps;
        let mut out = Vec::with_capacity(PICTURE_PARAMETER_BYTES);
        word(&mut out, s.pic_width_in_min_cbs as u16);
        word(&mut out, s.pic_height_in_min_cbs as u16);
        word(
            &mut out,
            u16::from(s.chroma_format_idc)
                | (u16::from(s.separate_colour_plane_flag) << 2)
                | (u16::from(s.bit_depth_luma - 8) << 3)
                | (u16::from(s.bit_depth_chroma - 8) << 6)
                | (u16::from(s.log2_max_poc_lsb - 4) << 9),
        );
        out.extend_from_slice(&[
            current,
            (self.dpb_size - 1) as u8,
            s.log2_min_cb_size - 3,
            s.log2_ctb_size - s.log2_min_cb_size,
            s.log2_min_tb_size - 2,
            s.log2_max_tb_size - s.log2_min_tb_size,
            s.max_transform_hierarchy_depth_inter,
            s.max_transform_hierarchy_depth_intra,
            s.st_rps.len() as u8,
            s.lt_ref_pics.len() as u8,
            p.num_ref_idx_l0_default_active - 1,
            p.num_ref_idx_l1_default_active - 1,
            (p.init_qp - 26) as i8 as u8,
            self.reference_rps_delta_pocs,
        ]);
        word(&mut out, self.header.st_rps_bits as u16);
        word(&mut out, 0);
        let tools = flag(s.scaling_list_enabled, 0)
            | flag(s.amp_enabled, 1)
            | flag(s.sao_enabled, 2)
            | flag(s.pcm_enabled, 3)
            | (u32::from(s.pcm_bit_depth_luma.saturating_sub(1)) << 4)
            | (u32::from(s.pcm_bit_depth_chroma.saturating_sub(1)) << 8)
            | (u32::from(s.log2_min_pcm_cb_size.saturating_sub(3)) << 12)
            | (u32::from(
                s.log2_max_pcm_cb_size
                    .saturating_sub(s.log2_min_pcm_cb_size),
            ) << 14)
            | flag(s.pcm_loop_filter_disabled, 16)
            | flag(s.long_term_ref_pics_present, 17)
            | flag(s.temporal_mvp_enabled, 18)
            | flag(s.strong_intra_smoothing_enabled, 19)
            | flag(p.dependent_slice_segments_enabled, 20)
            | flag(p.output_flag_present, 21)
            | (u32::from(p.num_extra_slice_header_bits) << 22)
            | flag(p.sign_data_hiding_enabled, 25)
            | flag(p.cabac_init_present, 26);
        dword(&mut out, tools);
        let settings = flag(p.constrained_intra_pred, 0)
            | flag(p.transform_skip_enabled, 1)
            | flag(p.cu_qp_delta_enabled, 2)
            | flag(p.slice_chroma_qp_offsets_present, 3)
            | flag(p.weighted_pred, 4)
            | flag(p.weighted_bipred, 5)
            | flag(p.transquant_bypass_enabled, 6)
            | flag(p.tiles_enabled, 7)
            | flag(p.entropy_coding_sync_enabled, 8)
            | flag(p.uniform_spacing, 9)
            | flag(p.loop_filter_across_tiles_enabled, 10)
            | flag(p.loop_filter_across_slices_enabled, 11)
            | flag(p.deblocking_filter_override_enabled, 12)
            | flag(p.deblocking_filter_disabled, 13)
            | flag(p.lists_modification_present, 14)
            | flag(p.slice_segment_header_extension_present, 15)
            | flag(self.nal.nal_type.is_irap(), 16)
            | flag(self.nal.nal_type.is_idr(), 17)
            | flag(self.nal.nal_type.is_irap(), 18);
        dword(&mut out, settings);
        out.extend_from_slice(&[
            p.cb_qp_offset as i8 as u8,
            p.cr_qp_offset as i8 as u8,
            p.num_tile_columns.saturating_sub(1) as u8,
            p.num_tile_rows.saturating_sub(1) as u8,
        ]);
        for index in 0..19 {
            word(
                &mut out,
                p.column_widths
                    .get(index)
                    .map_or(0, |value| (value - 1) as u16),
            );
        }
        for index in 0..21 {
            word(
                &mut out,
                p.row_heights
                    .get(index)
                    .map_or(0, |value| (value - 1) as u16),
            );
        }
        out.extend_from_slice(&[
            p.diff_cu_qp_delta_depth,
            p.beta_offset_div2 as i8 as u8,
            p.tc_offset_div2 as i8 as u8,
            p.log2_parallel_merge_level - 2,
        ]);
        dword(&mut out, self.poc as u32);
        for index in 0..MAX_REFERENCES {
            out.push(surfaces.get(index).map_or(0xff, |surface| {
                surface.slot | if surface.long_term { 0x80 } else { 0 }
            }));
        }
        out.push(0);
        for index in 0..MAX_REFERENCES {
            dword(
                &mut out,
                surfaces.get(index).map_or(0, |surface| surface.poc as u32),
            );
        }
        for kind in [
            ReferenceKind::Before,
            ReferenceKind::After,
            ReferenceKind::LongTerm,
        ] {
            let mut indices = Vec::new();
            for reference in self
                .references
                .iter()
                .filter(|reference| reference.kind == kind)
            {
                let index = surfaces
                    .iter()
                    .position(|surface| reference.matches(surface.poc))
                    .ok_or_else(|| Error::invalid("DXVA current reference is unavailable"))?;
                indices.push(index as u8);
            }
            if indices.len() > 8 {
                return Err(Error::unsupported(
                    "DXVA current reference list exceeds eight entries",
                ));
            }
            indices.resize(8, 0xff);
            out.extend(indices);
        }
        word(&mut out, 0);
        word(&mut out, 0);
        dword(&mut out, feedback);
        debug_assert_eq!(out.len(), PICTURE_PARAMETER_BYTES);
        Ok(out)
    }

    pub fn quantization(&self) -> Option<Vec<u8>> {
        if !self.sps.scaling_list_enabled {
            return None;
        }
        let lists = self
            .pps
            .scaling_list
            .as_ref()
            .or(self.sps.scaling_list.as_ref())?;
        let mut bytes = Vec::with_capacity(QUANTIZATION_BYTES);
        for matrix in &lists.lists[0] {
            bytes.extend_from_slice(&matrix[..16]);
        }
        for size in 1..=2 {
            for matrix in &lists.lists[size] {
                bytes.extend_from_slice(matrix);
            }
        }
        for index in [0, 3] {
            bytes.extend_from_slice(&lists.lists[3][index]);
        }
        bytes.extend_from_slice(&lists.dc[0]);
        for index in [0, 3] {
            bytes.push(lists.dc[1][index]);
        }
        debug_assert_eq!(bytes.len(), QUANTIZATION_BYTES);
        Some(bytes)
    }

    pub fn slice_control(&self, padded_bytes: usize) -> Result<Vec<u8>> {
        if padded_bytes < self.bitstream.len() || padded_bytes - self.bitstream.len() >= 128 {
            return Err(Error::invalid("DXVA bitstream padding"));
        }
        let mut bytes = Vec::with_capacity(self.slices.len() * 10);
        for (index, slice) in self.slices.iter().enumerate() {
            dword(&mut bytes, slice.offset);
            dword(
                &mut bytes,
                slice.length
                    + if index + 1 == self.slices.len() {
                        (padded_bytes - self.bitstream.len()) as u32
                    } else {
                        0
                    },
            );
            word(&mut bytes, 0);
        }
        Ok(bytes)
    }
}
fn flag(value: bool, shift: u32) -> u32 {
    u32::from(value) << shift
}
fn word(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn dword(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn reference_rps_count(
    bytes: &[u8],
    sps: &Sps,
    pps: &Pps,
    nal: NalHeader,
) -> Result<u8> {
    if nal.nal_type.is_idr() || sps.st_rps.is_empty() {
        return Ok(0);
    }
    let mut r = BitReader::new(bytes);
    if !r.read_flag()? {
        return Err(Error::invalid("reference RPS requires the first slice"));
    }
    if nal.nal_type.is_irap() {
        r.read_flag()?;
    }
    r.read_ue_max(63)?;
    for _ in 0..pps.num_extra_slice_header_bits {
        r.read_flag()?;
    }
    r.read_ue_max(2)?;
    if pps.output_flag_present {
        r.read_flag()?;
    }
    if sps.separate_colour_plane_flag {
        r.read_bits(2)?;
    }
    r.read_bits(sps.log2_max_poc_lsb as u32)?;
    if r.read_flag()? || !r.read_flag()? {
        return Ok(0);
    }
    let delta = r.read_ue_max(sps.st_rps.len() as u32 - 1)? as usize;
    Ok(sps.st_rps[sps.st_rps.len() - delta - 1].num_delta_pocs() as u8)
}
