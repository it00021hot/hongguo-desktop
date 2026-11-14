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

/// 把一个 MP4 样本（长度前缀 NAL 串）转成 Annex-B。
/// `length_size` 来自 [`super::length_size_from_extradata`]；未知时传 4（librempeg 默认）。
pub fn sample_to_annexb(sample: &[u8], length_size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(sample.len().saturating_add(16));
    append_annexb(sample, length_size, &mut out);
    out
}

/// 同 [`sample_to_annexb`]，但追加到调用方的缓冲末尾（不清空），解码循环用它复用一块缓冲。
pub(crate) fn append_annexb(sample: &[u8], length_size: usize, out: &mut Vec<u8>) {
    let _ = append_annexb_checked(sample, length_size, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_samples_reject_incomplete_or_unbounded_nal_units() {
        assert!(sample_to_annexb_checked(&[], 4).is_err());
        assert!(sample_to_annexb_checked(&[0, 0, 0], 4).is_err());
        assert!(sample_to_annexb_checked(&[0, 0, 0, 3, 1], 4).is_err());
        assert!(sample_to_annexb_checked(&[255; 8], 8).is_err());
        assert_eq!(
            sample_to_annexb_checked(&[0, 0, 0, 1, 0x11], 4).unwrap(),
            [0, 0, 0, 1, 0x11]
        );
    }

    #[test]
    fn splits_length_prefixed_nals() {
        let mut sample = Vec::new();
        sample.extend_from_slice(&3u32.to_be_bytes());
        sample.extend_from_slice(&[0x11, 0x22, 0x33]);
        sample.extend_from_slice(&2u32.to_be_bytes());
        sample.extend_from_slice(&[0xaa, 0xbb]);
        let out = sample_to_annexb(&sample, 4);
        assert_eq!(out, [0, 0, 0, 1, 0x11, 0x22, 0x33, 0, 0, 0, 1, 0xaa, 0xbb]);
    }

    #[test]
    fn append_keeps_existing_prefix() {
        let mut sample = Vec::new();
        sample.extend_from_slice(&1u32.to_be_bytes());
        sample.push(0x7f);
        let mut out = vec![9u8, 9];
        append_annexb(&sample, 4, &mut out);
        assert_eq!(out, [9, 9, 0, 0, 0, 1, 0x7f]);
    }
}
