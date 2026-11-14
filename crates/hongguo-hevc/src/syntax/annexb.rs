use super::*;

const MAX_NALS: usize = 4096;

pub(super) fn split(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    if bytes.len() > MAX_ACCESS_UNIT_BYTES {
        return Err(Error::invalid("oversized HEVC access unit"));
    }
    let mut units = Vec::new();
    let mut previous = None;
    let mut offset = 0;
    while offset + 3 <= bytes.len() {
        if bytes[offset..offset + 3] == [0, 0, 1] {
            if let Some(start) = previous {
                push(&mut units, bytes, start, offset)?;
            }
            previous = Some(offset + 3);
            offset += 3;
        } else {
            offset += 1;
        }
    }
    if let Some(start) = previous {
        push(&mut units, bytes, start, bytes.len())?;
    }
    if units.is_empty() {
        return Err(Error::invalid("HEVC access unit has no Annex-B NAL"));
    }
    Ok(units)
}
fn push<'a>(
    units: &mut Vec<&'a [u8]>,
    bytes: &'a [u8],
    start: usize,
    mut end: usize,
) -> Result<()> {
    while end > start && bytes[end - 1] == 0 {
        end -= 1;
    }
    if start == end {
        return Err(Error::invalid("empty HEVC NAL"));
    }
    if units.len() == MAX_NALS {
        return Err(Error::invalid("HEVC NAL count exceeds access-unit budget"));
    }
    units.push(&bytes[start..end]);
    Ok(())
}
