use crate::{ContentLightMetadata, MasteringDisplayMetadata};

impl MasteringDisplayMetadata {
    /// HEVC SEI 137 and MP4 mdcv share the big-endian G,B,R payload.
    pub fn from_payload(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; 24] = bytes.try_into().ok()?;
        let word = |at| u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        Some(Self {
            primaries: [[word(8), word(10)], [word(0), word(2)], [word(4), word(6)]],
            white_point: [word(12), word(14)],
            max_luminance: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            min_luminance: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        })
    }
}
impl ContentLightMetadata {
    pub fn from_payload(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; 4] = bytes.try_into().ok()?;
        Some(Self {
            max_content_light_level: u16::from_be_bytes([bytes[0], bytes[1]]),
            max_pic_average_light_level: u16::from_be_bytes([bytes[2], bytes[3]]),
        })
    }
}
