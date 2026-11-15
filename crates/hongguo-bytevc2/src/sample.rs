//! 长度前缀样本 → Annex-B。librempeg `cbs_bvc2_split_fragment` 固定 `nal_length_size = 4`。

const START_CODE: [u8; 4] = [0, 0, 0, 1];
use hongguo_common::{AppError, AppResult};

pub fn sample_to_annexb_checked(sample: &[u8], length_size: usize) -> AppResult<Vec<u8>> {
    let mut output = Vec::with_capacity(sample.len().saturating_add(16));
    append_annexb_checked(sample, length_size, &mut output)?;
    Ok(output)
}

pub(crate) fn append_annexb_checked(
    sample: &[u8],
    length_size: usize,
    output: &mut Vec<u8>,
) -> AppResult<()> {
    if !(1..=8).contains(&length_size) {
        return Err(AppError::Media("invalid ByteVC2 NAL length size".into()));
    }
    let mut cursor = 0usize;
    while cursor < sample.len() {
        let prefix = sample
            .get(cursor..cursor + length_size)
            .ok_or_else(|| AppError::Media("truncated ByteVC2 NAL length".into()))?;
        let length = prefix
            .iter()
            .try_fold(0usize, |length, byte| {
                length
                    .checked_mul(256)
                    .and_then(|length| length.checked_add(usize::from(*byte)))
            })
            .ok_or_else(|| AppError::Media("ByteVC2 NAL length overflow".into()))?;
        cursor += length_size;
        let end = cursor
            .checked_add(length)
            .ok_or_else(|| AppError::Media("ByteVC2 NAL length overflow".into()))?;
        let payload = sample
            .get(cursor..end)
            .filter(|payload| !payload.is_empty())
            .ok_or_else(|| AppError::Media("truncated or empty ByteVC2 NAL".into()))?;
        output.extend_from_slice(&START_CODE);
        output.extend_from_slice(payload);
        cursor = end;
    }
    if sample.is_empty() {
        return Err(AppError::Media("empty ByteVC2 access unit".into()));
    }
    Ok(())
}
