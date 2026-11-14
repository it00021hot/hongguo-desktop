mod sampling;
mod shader;
#[cfg(test)]
mod tests;

use hongguo_media_types::{ColorMetadata, FrameDescriptor, FrameImage};
use hongguo_video_color::{
    ColorTransform, DisplayTarget, LUT_HEIGHT, LUT_WIDTH, OutputEncoding, Primaries,
};
use nsh_platform_windows::{OutputColorInfo, OutputColorMode};
use windows::Win32::Graphics::{
    Direct2D::ID2D1DeviceContext,
    Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC,
        D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIEWPORT,
        ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader, ID3D11RenderTargetView,
        ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
    },
    Dxgi::Common::{DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_SAMPLE_DESC},
};

use crate::{
    VideoError,
    input::{InputGeometry, InputKind, TextureInput},
};
use std::sync::Arc;

type ColorKey = (ColorMetadata, u8, usize, DisplayTarget);
const GEOMETRY_BYTES: u32 = 64;

pub(crate) struct Conversion {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    input: TextureInput,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    geometry: ID3D11Buffer,
    color: Option<ID3D11Buffer>,
    transform: Option<Arc<ColorTransform>>,
    color_key: Option<ColorKey>,
    lut: Option<ID3D11ShaderResourceView>,
    display: Option<OutputColorInfo>,
}

impl Conversion {
    pub fn new(device: &ID3D11Device) -> Result<Self, VideoError> {
        let (mut context, mut vertex, mut pixel) = (None, None, None);
        let modern = unsafe { device.GetFeatureLevel() }.0
            >= windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0.0;
        let vertex_code = shader::compile(c"vertex", if modern { c"vs_5_0" } else { c"vs_4_0" })?;
        let pixel_code = shader::compile(c"pixel", if modern { c"ps_5_0" } else { c"ps_4_0" })?;
        // SAFETY: shader bytecode and all out pointers remain valid through creation.
        unsafe {
            device
                .CreateDeferredContext(0, Some(&mut context))
                .map_err(error)?;
            device
                .CreateVertexShader(shader::bytes(&vertex_code), None, Some(&mut vertex))
                .map_err(error)?;
            device
                .CreatePixelShader(shader::bytes(&pixel_code), None, Some(&mut pixel))
                .map_err(error)?;
        }
        Ok(Self {
            device: device.clone(),
            context: context.ok_or_else(|| error("missing deferred context"))?,
            input: TextureInput::new(device),
            vertex: vertex.ok_or_else(|| error("missing vertex shader"))?,
            pixel: pixel.ok_or_else(|| error("missing color shader"))?,
            geometry: buffer(device, GEOMETRY_BYTES)?,
            color: None,
            transform: None,
            color_key: None,
            lut: None,
            display: None,
        })
    }

    pub fn render(
        &mut self,
        dc: &ID2D1DeviceContext,
        frame: &FrameImage,
        output: &ID3D11RenderTargetView,
        working: Option<&ID3D11RenderTargetView>,
        size: (u32, u32),
        display: OutputColorInfo,
    ) -> Result<(), VideoError> {
        // SAFETY: the deferred context is private; it never alters NSH's live render state.
        unsafe { self.context.ClearState() };
        let recorded = self.record(frame, output, working, size, display);
        let mut commands = None;
        unsafe { self.context.FinishCommandList(false, Some(&mut commands)) }.map_err(error)?;
        recorded?;
        let commands = commands.ok_or_else(|| error("missing color conversion command list"))?;
        // Flush NSH before foreign GPU work, then restore every immediate-context state D2D owns.
        unsafe {
            dc.Flush(None, None).map_err(error)?;
            self.device
                .GetImmediateContext()
                .map_err(error)?
                .ExecuteCommandList(&commands, true);
        }
        self.display = Some(display);
        Ok(())
    }

    pub fn display_changed(&self, _frame: &FrameImage, display: OutputColorInfo) -> bool {
        self.display != Some(display)
    }
    pub fn transform(&self) -> Arc<ColorTransform> {
        self.transform
            .as_ref()
            .expect("prepared color transform")
            .clone()
    }

    fn record(
        &mut self,
        frame: &FrameImage,
        output: &ID3D11RenderTargetView,
        working: Option<&ID3D11RenderTargetView>,
        size: (u32, u32),
        display: OutputColorInfo,
    ) -> Result<(), VideoError> {
        let descriptor = frame.descriptor();
        let geometry = self.input.prepare(&self.context, frame)?;
        let params = geometry_bytes(geometry, &descriptor)?;
        self.prepare_color(&descriptor, display)?;
        let views = self.input.views();
        let textures = [
            views.first().cloned(),
            views.get(1).cloned(),
            views.get(2).cloned(),
            self.lut.clone(),
        ];
        unsafe {
            self.context
                .UpdateSubresource(&self.geometry, 0, None, params.as_ptr().cast(), 0, 0);
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.VSSetShader(&self.vertex, None);
            self.context.PSSetShader(&self.pixel, None);
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.geometry.clone()), self.color.clone()]));
            self.context.PSSetShaderResources(0, Some(&textures));
            self.context
                .OMSetRenderTargets(Some(&[Some(output.clone()), working.cloned()]), None);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: size.0 as f32,
                Height: size.1 as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            self.context.Draw(3, 0);
        }
        Ok(())
    }

    fn prepare_color(
        &mut self,
        frame: &FrameDescriptor,
        display: OutputColorInfo,
    ) -> Result<(), VideoError> {
        let linear = display.mode == OutputColorMode::ScRgb;
        // SDR is a defined BT.709/sRGB mapping target, independent of unreported
        // physical display measurements. HDR never invents a measured white.
        let white = if linear {
            display
                .sdr_white_level_nits
                .ok_or_else(|| error("display did not report its SDR white level"))?
        } else {
            100.0
        };
        let gamut = display.chromaticities;
        let mut primaries = if linear {
            Primaries {
                red: gamut.red.map(f64::from),
                green: gamut.green.map(f64::from),
                blue: gamut.blue.map(f64::from),
                white: gamut.white.map(f64::from),
            }
        } else {
            Primaries::BT709
        };
        if [
            primaries.red,
            primaries.green,
            primaries.blue,
            primaries.white,
        ]
        .iter()
        .any(|point| {
            !point[0].is_finite() || !point[1].is_finite() || point[0] <= 0.0 || point[1] <= 0.0
        }) {
            primaries = Primaries::BT709;
        }
        let peak = if linear && display.hdr_enabled {
            display.luminance.peak_nits.unwrap_or(white).max(white)
        } else {
            white
        };
        let target = DisplayTarget {
            primaries,
            sdr_white_nits: white,
            peak_nits: peak,
            encoding: if linear {
                OutputEncoding::LinearScRgb
            } else {
                OutputEncoding::Srgb
            },
            gamut_mapping: true,
        };
        let key = (frame.color.clone(), frame.bit_depth, frame.height, target);
        if self.color_key.as_ref() == Some(&key) {
            return Ok(());
        }
        let transform = ColorTransform::new(&frame.color, frame.bit_depth, frame.height, target)
            .map_err(error)?;
        if transform.assumed_metadata {
            log::warn!(
                "video color metadata is incomplete; applying the shared documented source assumptions"
            );
        }
        if (display.hdr_enabled && display.luminance.peak_nits.is_none())
            || primaries == Primaries::BT709 && gamut.red == [0.0, 0.0]
        {
            log::warn!(
                "display color metadata is incomplete; limiting its mapped gamut/peak to the known SDR reference"
            );
        }
        let data = transform.lut_bytes();
        let descriptor = D3D11_TEXTURE2D_DESC {
            Width: LUT_WIDTH,
            Height: LUT_HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            SysMemPitch: LUT_WIDTH * 16,
            SysMemSlicePitch: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&descriptor, Some(&initial), Some(&mut texture))
        }
        .map_err(error)?;
        let texture = texture.ok_or_else(|| error("missing color LUT texture"))?;
        let mut view = None;
        unsafe {
            self.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))
        }
        .map_err(error)?;
        self.lut = Some(view.ok_or_else(|| error("missing color LUT view"))?);
        let bytes = transform.params.bytes();
        let color = buffer(&self.device, u32::try_from(bytes.len()).map_err(error)?)?;
        unsafe {
            self.context
                .UpdateSubresource(&color, 0, None, bytes.as_ptr().cast(), 0, 0)
        };
        self.color = Some(color);
        self.transform = Some(Arc::new(transform));
        self.color_key = Some(key);
        Ok(())
    }
}

fn geometry_bytes(input: InputGeometry, frame: &FrameDescriptor) -> Result<Vec<u8>, VideoError> {
    let crop = frame.crop;
    if crop.width == 0
        || crop.height == 0
        || crop
            .x
            .checked_add(crop.width)
            .is_none_or(|end| end > frame.width)
        || crop
            .y
            .checked_add(crop.height)
            .is_none_or(|end| end > frame.height)
        || !matches!(frame.rotation_degrees, 0 | 90 | 180 | 270)
    {
        return Err(error("invalid native visible crop or rotation"));
    }
    let (step_x, step_y) = (
        if input.chroma_width == input.width {
            1.0
        } else {
            2.0
        },
        if input.chroma_height == input.height {
            1.0
        } else {
            2.0
        },
    );
    let [phase_x, phase_y] = sampling::chroma_phase(frame.color.chroma_location, [step_x, step_y]);
    let values = [
        crop.x as f32,
        crop.y as f32,
        crop.width as f32,
        crop.height as f32,
        input.width as f32,
        input.height as f32,
        input.chroma_width as f32,
        input.chroma_height as f32,
    ];
    let flags = [
        match input.kind {
            InputKind::Planar => 0,
            InputKind::Interleaved => 1,
            InputKind::Monochrome => 2,
        },
        input.shift,
        1_u32 << (frame.bit_depth - 1),
        u32::from(frame.rotation_degrees / 90),
    ];
    Ok(values
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .chain(flags.into_iter().flat_map(u32::to_le_bytes))
        .chain(
            [step_x, step_y, phase_x, phase_y]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        )
        .collect())
}

fn buffer(device: &ID3D11Device, bytes: u32) -> Result<ID3D11Buffer, VideoError> {
    let descriptor = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut result = None;
    unsafe { device.CreateBuffer(&descriptor, None, Some(&mut result)) }.map_err(error)?;
    result.ok_or_else(|| error("missing color conversion uniform buffer"))
}

fn error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(
        hongguo_nsh_video_api::ErrorCode::FrameUnavailable,
        error.to_string(),
    )
}
