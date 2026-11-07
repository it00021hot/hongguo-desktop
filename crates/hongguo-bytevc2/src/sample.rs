//! 长度前缀样本 → Annex-B。librempeg `cbs_bvc2_split_fragment` 固定 `nal_length_size = 4`。

const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// 把一个 MP4 样本（长度前缀 NAL 串）转成 Annex-B。
/// `length_size` 来自 [`super::length_size_from_extradata`]；未知时传 4（librempeg 默认）。
pub fn sample_to_annexb(sample: &[u8], length_size: usize) -> Vec<u8> {
    let length_size = length_size.max(1);
    let mut out = Vec::with_capacity(sample.len().saturating_add(16));
    let mut pos = 0usize;
    while pos + length_size <= sample.len() {
        let mut len = 0usize;
        for i in 0..length_size {
            len = (len << 8) | sample[pos + i] as usize;
        }
        pos += length_size;
        if len == 0 || pos + len > sample.len() {
            break;
        }
        out.extend_from_slice(&START_CODE);
        out.extend_from_slice(&sample[pos..pos + len]);
        pos += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
