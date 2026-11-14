use hongguo_media_types::{DecodeDevice, HardwareKind};
use std::sync::Arc;
use windows::{Win32::Graphics::Direct3D11::ID3D11Device, core::Interface};

pub(crate) struct DeviceLease {
    device: ID3D11Device,
    generation: u64,
}

impl DeviceLease {
    pub fn new(device: &ID3D11Device, generation: u64) -> Arc<Self> {
        Arc::new(Self {
            device: device.clone(),
            generation,
        })
    }
}

// SAFETY: this lease owns the COM reference and the actual NSH device uses D3D11 multithread protection.
unsafe impl DecodeDevice for DeviceLease {
    fn kind(&self) -> HardwareKind {
        HardwareKind::D3d11
    }
    fn device(&self) -> usize {
        self.device.as_raw() as usize
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}
