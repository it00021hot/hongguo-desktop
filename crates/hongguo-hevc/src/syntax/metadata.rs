use super::*;
use hongguo_media_types::{ContentLightMetadata, MasteringDisplayMetadata};
use rusty_h265::ps::parse_st_ref_pic_set;

pub(super) fn sps(bytes: &[u8], sps: &Sps) -> Result<ColorMetadata> {
    let mut r = BitReader::new(bytes);
    skip(&mut r, 8 + 96)?;
    let count = sps.max_sub_layers_minus1 as usize;
    let mut layers = Vec::new();
    for _ in 0..count {
        layers.push((r.read_flag()?, r.read_flag()?));
    }
    if count > 0 {
        skip(&mut r, (8 - count) * 2)?;
    }
    for (profile, level) in layers {
        skip(&mut r, usize::from(profile) * 88 + usize::from(level) * 8)?;
    }
    r.read_ue()?;
    if r.read_ue()? == 3 {
        r.read_flag()?;
    }
    r.read_ue()?;
    r.read_ue()?;
    if r.read_flag()? {
        for _ in 0..4 {
            r.read_ue()?;
        }
    }
    for _ in 0..3 {
        r.read_ue()?;
    }
    let start = if r.read_flag()? { 0 } else { count };
    for _ in start..=count {
        for _ in 0..3 {
            r.read_ue()?;
        }
    }
    for _ in 0..6 {
        r.read_ue()?;
    }
    if r.read_flag()? && r.read_flag()? {
        skip_scaling(&mut r)?;
    }
    r.read_flag()?;
    r.read_flag()?;
    if r.read_flag()? {
        skip(&mut r, 8)?;
        r.read_ue()?;
        r.read_ue()?;
        r.read_flag()?;
    }
    let sets = r.read_ue_max(64)? as usize;
    for index in 0..sets {
        parse_st_ref_pic_set(&mut r, index, &sps.st_rps[..index], false)?;
    }
    if r.read_flag()? {
        let entries = r.read_ue_max(32)?;
        for _ in 0..entries {
            skip(&mut r, sps.log2_max_poc_lsb as usize + 1)?;
        }
    }
    r.read_flag()?;
    r.read_flag()?;
    if !r.read_flag()? {
        return Ok(ColorMetadata::default());
    }
    vui(&mut r)
}

pub(super) fn vui(r: &mut BitReader<'_>) -> Result<ColorMetadata> {
    let mut color = ColorMetadata::default();
    if r.read_flag()? {
        let id = r.read_bits(8)?;
        let ratios = [
            (0, 0),
            (1, 1),
            (12, 11),
            (10, 11),
            (16, 11),
            (40, 33),
            (24, 11),
            (20, 11),
            (32, 11),
            (80, 33),
            (18, 11),
            (15, 11),
            (64, 33),
            (160, 99),
            (4, 3),
            (3, 2),
            (2, 1),
        ];
        let ratio = if id == 255 {
            (r.read_bits(16)?, r.read_bits(16)?)
        } else {
            ratios.get(id as usize).copied().unwrap_or((0, 0))
        };
        if ratio.0 > 0 && ratio.1 > 0 {
            color.sample_aspect_ratio = Some(ratio);
        }
    }
    if r.read_flag()? {
        r.read_flag()?;
    }
    if r.read_flag()? {
        r.read_bits(3)?;
        color.full_range = Some(r.read_flag()?);
        if r.read_flag()? {
            let mut component = || -> Result<Option<u8>> {
                let value = r.read_bits(8)? as u8;
                Ok((value != 2).then_some(value))
            };
            color.primaries = component()?;
            color.transfer = component()?;
            color.matrix = component()?;
        }
    }
    if r.read_flag()? {
        let top = r.read_ue_max(5)?;
        let bottom = r.read_ue_max(5)?;
        if top != bottom {
            return Err(Error::unsupported("field-dependent HEVC chroma location"));
        }
        color.chroma_location = Some(top as u8 + 1);
    }
    r.read_flag()?;
    if r.read_flag()? {
        return Err(Error::unsupported("interlaced HEVC VUI"));
    }
    Ok(color)
}

fn skip(r: &mut BitReader<'_>, mut bits: usize) -> Result<()> {
    while bits > 0 {
        let count = bits.min(32);
        r.read_bits(count as u32)?;
        bits -= count;
    }
    Ok(())
}
fn skip_scaling(r: &mut BitReader<'_>) -> Result<()> {
    for size in 0..4 {
        for _ in (0..6).step_by(if size == 3 { 3 } else { 1 }) {
            if !r.read_flag()? {
                r.read_ue()?;
            } else {
                if size > 1 {
                    r.read_se()?;
                }
                for _ in 0..(1_usize << (4 + 2 * size)).min(64) {
                    r.read_se()?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn sei(bytes: &[u8], color: &mut ColorMetadata) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() && bytes[offset..] != [0x80] {
        let kind = extended(bytes, &mut offset)?;
        let size = extended(bytes, &mut offset)?;
        let end = offset
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::invalid("truncated HEVC SEI payload"))?;
        let payload = &bytes[offset..end];
        if kind == 137 {
            color.mastering_display = Some(
                MasteringDisplayMetadata::from_payload(payload)
                    .ok_or_else(|| Error::invalid("HEVC mastering display SEI length"))?,
            );
        } else if kind == 144 {
            color.content_light = Some(
                ContentLightMetadata::from_payload(payload)
                    .ok_or_else(|| Error::invalid("HEVC content light SEI length"))?,
            );
        }
        offset = end;
    }
    Ok(())
}
fn extended(bytes: &[u8], offset: &mut usize) -> Result<usize> {
    let mut value: usize = 0;
    loop {
        let byte = *bytes
            .get(*offset)
            .ok_or_else(|| Error::invalid("truncated HEVC SEI header"))?;
        *offset += 1;
        value = value
            .checked_add(byte as usize)
            .ok_or_else(|| Error::invalid("oversized HEVC SEI"))?;
        if byte != 255 {
            return Ok(value);
        }
    }
}
