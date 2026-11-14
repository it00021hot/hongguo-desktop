use crate::FrameImage;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone, Debug)]
pub struct HevcDecoderConfig {
    pub initial_access_unit: Vec<u8>,
    pub timescale: u32,
    pub cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub struct DecodedVideoFrame {
    pub image: FrameImage,
    pub pts_secs: f64,
}

/// Created and driven on the decoding worker; native pictures retain their surfaces.
pub trait VideoDecoder {
    fn decode(&mut self, annex_b: &[u8], pts: i64) -> Result<Vec<DecodedVideoFrame>, String>;
    fn drain(&mut self) -> Result<Vec<DecodedVideoFrame>, String>;
}
