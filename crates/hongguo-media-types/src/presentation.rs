use std::{fmt, sync::Arc};

use crate::{ColorMetadata, CropRect, NativeFrame};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HardwareKind {
    VideoToolbox,
    D3d11,
    MediaCodec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareHandle {
    pub kind: HardwareKind,
    pub object: usize,
    pub subresource: usize,
}

#[derive(Clone, Debug)]
pub struct FrameDescriptor {
    pub width: usize,
    pub height: usize,
    pub bit_depth: u8,
    pub crop: CropRect,
    pub rotation_degrees: u16,
    pub color: ColorMetadata,
}

/// # Safety
/// The handle must name the declared native object and remain live until the lease is dropped.
pub unsafe trait HardwareFrame: Send + Sync {
    fn handle(&self) -> HardwareHandle;
    fn descriptor(&self) -> FrameDescriptor;
    fn present_at(&self, _monotonic_ns: i64) -> Result<(), String> {
        Err("native frame does not support timed surface presentation".into())
    }
}

#[derive(Clone)]
pub enum FrameImage {
    Cpu(Arc<NativeFrame>),
    Hardware(Arc<dyn HardwareFrame>),
}

impl FrameImage {
    pub fn descriptor(&self) -> FrameDescriptor {
        match self {
            Self::Cpu(frame) => FrameDescriptor {
                width: frame.width,
                height: frame.height,
                bit_depth: frame.bit_depth,
                crop: frame.crop,
                rotation_degrees: frame.rotation_degrees,
                color: frame.color.clone(),
            },
            Self::Hardware(frame) => frame.descriptor(),
        }
    }
}

impl fmt::Debug for FrameImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu(frame) => formatter.debug_tuple("Cpu").field(frame).finish(),
            Self::Hardware(frame) => formatter
                .debug_tuple("Hardware")
                .field(&frame.handle())
                .finish(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PresentationFrame {
    pub serial: u64,
    pub generation: u64,
    pub pts_secs: f64,
    pub duration_secs: f64,
    pub image: FrameImage,
}

/// # Safety
/// Device and surface handles must stay valid for every clone of this lease.
pub unsafe trait DecodeDevice: Send + Sync {
    fn kind(&self) -> HardwareKind;
    fn device(&self) -> usize;
    fn surface(&self) -> usize {
        0
    }
    fn generation(&self) -> u64;
}
