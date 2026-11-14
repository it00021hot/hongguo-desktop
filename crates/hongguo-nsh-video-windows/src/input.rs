mod layout;

use hongguo_media_types::{ChromaSampling, FrameImage, HardwareFrame, HardwareKind, NativeFrame};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
            ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_FORMAT_R8_UINT,
            DXGI_FORMAT_R8G8_UINT, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R16G16_UINT, DXGI_SAMPLE_DESC,
        },
    },
    core::Interface,
};

use crate::VideoError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputKind {
    Planar,
    Interleaved,
    Monochrome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputGeometry {
    pub kind: InputKind,
    pub width: u32,
    pub height: u32,
    pub chroma_width: u32,
    pub chroma_height: u32,
    pub shift: u32,
}

pub(crate) struct TextureInput {
    device: ID3D11Device,
    textures: Vec<ID3D11Texture2D>,
    views: Vec<ID3D11ShaderResourceView>,
    signature: Vec<(u32, u32, DXGI_FORMAT)>,
}

impl TextureInput {
    pub fn new(device: &ID3D11Device) -> Self {
        Self {
            device: device.clone(),
            textures: Vec::new(),
            views: Vec::new(),
            signature: Vec::new(),
        }
    }

    pub fn views(&self) -> &[ID3D11ShaderResourceView] {
        &self.views
    }

    pub fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        frame: &FrameImage,
    ) -> Result<InputGeometry, VideoError> {
        match frame {
            FrameImage::Cpu(frame) => self.upload(context, frame),
            FrameImage::Hardware(frame) => self.copy_native(context, frame.as_ref()),
        }
    }

    fn upload(
        &mut self,
        context: &ID3D11DeviceContext,
        frame: &NativeFrame,
    ) -> Result<InputGeometry, VideoError> {
        let count = if frame.chroma == ChromaSampling::Monochrome {
            1
        } else {
            3
        };
        if !(8..=16).contains(&frame.bit_depth) || frame.planes.len() != count {
            return Err(error("unsupported native CPU pixel layout"));
        }
        let chroma_size = match frame.chroma {
            ChromaSampling::Monochrome | ChromaSampling::Yuv444 => (frame.width, frame.height),
            ChromaSampling::Yuv422 => (frame.width.div_ceil(2), frame.height),
            ChromaSampling::Yuv420 => (frame.width.div_ceil(2), frame.height.div_ceil(2)),
        };
        for (index, plane) in frame.planes.iter().enumerate() {
            let expected = if index == 0 {
                (frame.width, frame.height)
            } else {
                chroma_size
            };
            if (plane.width, plane.height) != expected {
                return Err(error(
                    "native CPU planes disagree with the coded chroma dimensions",
                ));
            }
        }
        let bytes = if frame.bit_depth > 8 { 2 } else { 1 };
        let format = if bytes == 1 {
            DXGI_FORMAT_R8_UINT
        } else {
            DXGI_FORMAT_R16_UINT
        };
        let mut signature = Vec::with_capacity(count);
        for plane in &frame.planes {
            if plane.bytes_per_sample != bytes {
                return Err(error("native CPU bit depth disagrees with sample storage"));
            }
            let (width, height) = layout::plane_storage(
                plane.width,
                plane.height,
                plane.stride,
                bytes,
                plane.data.len(),
            )
            .map_err(error)?;
            signature.push((width, height, format));
        }
        if self.signature != signature {
            let mut textures = Vec::with_capacity(count);
            let mut views = Vec::with_capacity(count);
            for &(width, height, format) in &signature {
                let texture = texture(&self.device, width, height, format)?;
                views.push(view(&self.device, &texture, format)?);
                textures.push(texture);
            }
            self.textures = textures;
            self.views = views;
            self.signature = signature;
        }
        for (texture, plane) in self.textures.iter().zip(&frame.planes) {
            // SAFETY: plane_storage verified every accessed row and its pitch; samples stay at source precision.
            unsafe {
                context.UpdateSubresource(
                    texture,
                    0,
                    None,
                    plane.data.as_ptr().cast(),
                    plane.stride as u32,
                    0,
                )
            };
        }
        let luma = &frame.planes[0];
        let chroma = frame.planes.get(1).unwrap_or(luma);
        Ok(InputGeometry {
            kind: if count == 1 {
                InputKind::Monochrome
            } else {
                InputKind::Planar
            },
            width: luma.width as u32,
            height: luma.height as u32,
            chroma_width: chroma.width as u32,
            chroma_height: chroma.height as u32,
            shift: 0,
        })
    }

    fn copy_native(
        &mut self,
        context: &ID3D11DeviceContext,
        frame: &dyn HardwareFrame,
    ) -> Result<InputGeometry, VideoError> {
        let handle = frame.handle();
        if handle.kind != HardwareKind::D3d11 || handle.object == 0 {
            return Err(error(
                "a Windows native frame requires a retained D3D11 texture",
            ));
        }
        let pointer = handle.object as *mut std::ffi::c_void;
        // SAFETY: HardwareFrame's lease guarantees a live Texture2D until the caller's GPU completion query retires it.
        let source = unsafe { ID3D11Texture2D::from_raw_borrowed(&pointer) }
            .ok_or_else(|| error("null decoder surface"))?;
        let mut description = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut description) };
        let descriptor = frame.descriptor();
        if unsafe { source.GetDevice() }.map_err(error)? != self.device
            || handle.subresource >= description.ArraySize as usize
            || description.MipLevels != 1
            || description.SampleDesc.Count != 1
            || descriptor.width > description.Width as usize
            || descriptor.height > description.Height as usize
        {
            return Err(error(
                "decoder surface does not match the drawing device or coded dimensions",
            ));
        }
        let (y_format, uv_format, shift) = match description.Format {
            DXGI_FORMAT_NV12 if descriptor.bit_depth == 8 => {
                (DXGI_FORMAT_R8_UINT, DXGI_FORMAT_R8G8_UINT, 0)
            }
            DXGI_FORMAT_P010 if descriptor.bit_depth == 10 => {
                (DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R16G16_UINT, 6)
            }
            _ => {
                return Err(error(
                    "D3D11 decoder surface is neither NV12 nor 10-bit P010",
                ));
            }
        };
        let signature = vec![(description.Width, description.Height, description.Format)];
        if self.signature != signature {
            let texture = texture(
                &self.device,
                description.Width,
                description.Height,
                description.Format,
            )?;
            self.views = vec![
                view(&self.device, &texture, y_format)?,
                view(&self.device, &texture, uv_format)?,
            ];
            self.textures = vec![texture];
            self.signature = signature;
        }
        // Decoder arrays need not be shader-bindable. The selected slice is copied only on the GPU.
        unsafe {
            context.CopySubresourceRegion(
                &self.textures[0],
                0,
                0,
                0,
                0,
                source,
                handle.subresource as u32,
                None,
            )
        };
        Ok(InputGeometry {
            kind: InputKind::Interleaved,
            width: descriptor.width as u32,
            height: descriptor.height as u32,
            chroma_width: (descriptor.width as u32).div_ceil(2),
            chroma_height: (descriptor.height as u32).div_ceil(2),
            shift,
        })
    }
}

fn texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> Result<ID3D11Texture2D, VideoError> {
    let descriptor = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let mut result = None;
    // SAFETY: the descriptor requests one uninitialized GPU-only shader texture; prepare writes it before use.
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut result)) }.map_err(error)?;
    result.ok_or_else(|| error("D3D11 returned no input texture"))
}

fn view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView, VideoError> {
    let descriptor = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    let mut result = None;
    // SAFETY: integer Y/UV views select the matching video plane without precision conversion.
    unsafe { device.CreateShaderResourceView(texture, Some(&descriptor), Some(&mut result)) }
        .map_err(error)?;
    result.ok_or_else(|| error("D3D11 returned no input plane view"))
}

fn error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(
        hongguo_nsh_video_api::ErrorCode::FrameFormatUnsupported,
        error.to_string(),
    )
}
