use super::*;
use hongguo_media_types::{FrameDescriptor, HardwareFrame, HardwareHandle, HardwareKind};
use std::sync::Weak;

pub(super) struct Surface {
    texture: ID3D11Texture2D,
    index: u8,
    subresource: u32,
    descriptor: FrameDescriptor,
}
impl Surface {
    pub(super) fn slot(&self) -> u8 {
        self.index
    }
}

// SAFETY: this frame owns the texture COM reference and the pool cannot reuse its slice while any lease exists.
unsafe impl HardwareFrame for Surface {
    fn handle(&self) -> HardwareHandle {
        HardwareHandle {
            kind: HardwareKind::D3d11,
            object: self.texture.as_raw() as usize,
            subresource: self.subresource as usize,
        }
    }
    fn descriptor(&self) -> FrameDescriptor {
        self.descriptor.clone()
    }
}

pub(super) struct Pool {
    slots: Vec<Slot>,
    coded: (usize, usize, u8),
}
struct Slot {
    texture: ID3D11Texture2D,
    view: ID3D11VideoDecoderOutputView,
    subresource: u32,
    lease: Weak<Surface>,
}
impl Pool {
    pub(super) fn new(
        device: &ID3D11Device,
        video: &ID3D11VideoDevice,
        stream: D3D11_VIDEO_DECODER_DESC,
        count: usize,
        individual: bool,
    ) -> Result<Self, String> {
        let description = D3D11_TEXTURE2D_DESC {
            Width: stream.SampleWidth.next_multiple_of(128),
            Height: stream.SampleHeight.next_multiple_of(128),
            MipLevels: 1,
            ArraySize: if individual { 1 } else { count as u32 },
            Format: stream.OutputFormat,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_DECODER.0 as u32,
            ..Default::default()
        };
        let array = if individual {
            None
        } else {
            Some(texture(device, &description)?)
        };
        let mut slots = Vec::with_capacity(count);
        for index in 0..count {
            let texture = match &array {
                Some(texture) => texture.clone(),
                None => texture(device, &description)?,
            };
            let subresource = if individual { 0 } else { index as u32 };
            let description = D3D11_VIDEO_DECODER_OUTPUT_VIEW_DESC {
                DecodeProfile: stream.Guid,
                ViewDimension: D3D11_VDOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_DECODER_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VDOV {
                        ArraySlice: subresource,
                    },
                },
            };
            let mut view = None;
            unsafe { video.CreateVideoDecoderOutputView(&texture, &description, Some(&mut view)) }
                .map_err(error)?;
            slots.push(Slot {
                texture,
                view: view.ok_or("missing HEVC decoder output view")?,
                subresource,
                lease: Weak::new(),
            });
        }
        Ok(Self {
            slots,
            coded: (
                stream.SampleWidth as usize,
                stream.SampleHeight as usize,
                if stream.OutputFormat == DXGI_FORMAT_P010 {
                    10
                } else {
                    8
                },
            ),
        })
    }
    pub(super) fn accepts(&self, descriptor: &FrameDescriptor) -> bool {
        self.coded == (descriptor.width, descriptor.height, descriptor.bit_depth)
    }
    pub(super) fn acquire(&mut self, descriptor: FrameDescriptor) -> Result<Arc<Surface>, String> {
        let index = self
            .slots
            .iter()
            .position(|slot| slot.lease.strong_count() == 0)
            .ok_or("all HEVC hardware surfaces are still referenced")?;
        let surface = Arc::new(Surface {
            texture: self.slots[index].texture.clone(),
            index: index as u8,
            subresource: self.slots[index].subresource,
            descriptor,
        });
        self.slots[index].lease = Arc::downgrade(&surface);
        Ok(surface)
    }
    pub(super) fn view(&self, slot: u8) -> &ID3D11VideoDecoderOutputView {
        &self.slots[slot as usize].view
    }
}

fn texture(
    device: &ID3D11Device,
    description: &D3D11_TEXTURE2D_DESC,
) -> Result<ID3D11Texture2D, String> {
    let mut texture = None;
    unsafe { device.CreateTexture2D(description, None, Some(&mut texture)) }.map_err(error)?;
    texture.ok_or("missing HEVC decoder texture".into())
}
