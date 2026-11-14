use std::sync::Arc;

mod presentation;
pub use presentation::{
    DecodeDevice, FrameDescriptor, FrameImage, HardwareFrame, HardwareHandle, HardwareKind,
    PresentationFrame,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromaSampling {
    Monochrome,
    Yuv420,
    Yuv422,
    Yuv444,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CropRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Debug)]
pub struct NativePlane {
    /// Unsigned samples, little endian when bytes_per_sample is two.
    pub data: Arc<[u8]>,
    pub stride: usize,
    pub width: usize,
    pub height: usize,
    pub bytes_per_sample: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasteringDisplayMetadata {
    /// Chromaticity coordinates in units of 1/50000.
    pub primaries: [[u16; 2]; 3],
    pub white_point: [u16; 2],
    /// Luminance in units of 1/10000 cd/m².
    pub max_luminance: u32,
    pub min_luminance: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentLightMetadata {
    pub max_content_light_level: u16,
    pub max_pic_average_light_level: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorMetadata {
    pub primaries: Option<u8>,
    pub transfer: Option<u8>,
    pub matrix: Option<u8>,
    pub full_range: Option<bool>,
    /// AVChromaLocation numbering: 1 left, 2 center, 3 top-left, through 6 bottom.
    pub chroma_location: Option<u8>,
    pub sample_aspect_ratio: Option<(u32, u32)>,
    pub mastering_display: Option<MasteringDisplayMetadata>,
    pub content_light: Option<ContentLightMetadata>,
}

#[derive(Clone, Debug)]
pub struct NativeFrame {
    pub width: usize,
    pub height: usize,
    pub bit_depth: u8,
    pub chroma: ChromaSampling,
    pub planes: Vec<NativePlane>,
    /// Signed presentation time in seconds; None means the source did not supply a timestamp.
    pub pts: Option<f64>,
    pub color: ColorMetadata,
    pub crop: CropRect,
    pub rotation_degrees: u16,
}
