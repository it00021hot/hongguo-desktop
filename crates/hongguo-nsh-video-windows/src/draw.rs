use crate::VideoError;
use hongguo_nsh_video_api::frames::PixelFrame;
use hongguo_nsh_video_api::{ErrorCode, FrameMetadata, ResizeMode};
use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
            D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR,
            ID2D1Bitmap1, ID2D1Device2, ID2D1DeviceContext,
        },
        Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, ID3D11Device, ID3D11Multithread, ID3D11Texture2D,
        },
        Dxgi::{
            Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
            IDXGISurface,
        },
    },
    core::Interface,
};

/// Obtains NSH's actual drawing device through public Direct2D/DXGI interfaces.
pub fn drawing_device(dc: &ID2D1DeviceContext) -> Result<ID3D11Device, VideoError> {
    // SAFETY: NativeComponent::draw receives a live D2D context on its COM thread.
    unsafe {
        let device: ID2D1Device2 = dc.GetDevice().map_err(error)?.cast().map_err(error)?;
        let device: ID3D11Device = device
            .GetDxgiDevice()
            .map_err(error)?
            .cast()
            .map_err(error)?;
        let multithread: ID3D11Multithread = device
            .GetImmediateContext()
            .map_err(error)?
            .cast()
            .map_err(error)?;
        let _ = multithread.SetMultithreadProtected(true);
        Ok(device)
    }
}

/// Always retained as a same-device raw fallback, independently of the WGPU device.
pub struct RawFrame {
    texture: ID3D11Texture2D,
    bitmap: ID2D1Bitmap1,
    size: (u32, u32),
    metadata: Option<FrameMetadata>,
}

impl RawFrame {
    pub fn new(dc: &ID2D1DeviceContext, width: u32, height: u32) -> Result<Self, VideoError> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(error("invalid native raw frame dimensions"));
        }
        let device = drawing_device(dc)?;
        let description = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        // SAFETY: The texture and bitmap both belong to the current D2D drawing device.
        let (texture, bitmap) = unsafe {
            let mut texture = None;
            device
                .CreateTexture2D(&description, None, Some(&mut texture))
                .map_err(error)?;
            let texture: ID3D11Texture2D =
                texture.ok_or_else(|| error("D3D11 returned no raw video texture"))?;
            let bitmap = bitmap(dc, &texture)?;
            (texture, bitmap)
        };
        Ok(Self {
            texture,
            bitmap,
            size: (width, height),
            metadata: None,
        })
    }

    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }
    pub fn metadata(&self) -> Option<FrameMetadata> {
        self.metadata
    }

    pub fn upload(&mut self, frame: &PixelFrame) -> Result<(), VideoError> {
        let row = frame
            .width
            .checked_mul(4)
            .ok_or_else(|| error("BGRA row overflow"))?;
        let length = u64::from(frame.stride) * u64::from(frame.height);
        if (frame.width, frame.height) != self.size
            || frame.stride < row
            || length > frame.pixels.len() as u64
        {
            return Err(error("BGRA frame storage does not match the raw texture"));
        }
        // SAFETY: Immutable BGRA storage covers every row. The DEFAULT texture and
        // its immediate context belong to this window's drawing device.
        unsafe {
            let context = self
                .texture
                .GetDevice()
                .map_err(error)?
                .GetImmediateContext()
                .map_err(error)?;
            context.UpdateSubresource(
                &self.texture,
                0,
                None,
                frame.pixels.as_ptr().cast(),
                frame.stride,
                0,
            );
        }
        let mut metadata = FrameMetadata::default();
        metadata.pts_ms = frame.pts_ms;
        metadata.width = frame.width;
        metadata.height = frame.height;
        self.metadata = Some(metadata);
        Ok(())
    }

    pub fn draw(
        &self,
        dc: &ID2D1DeviceContext,
        size: (f32, f32),
        mode: ResizeMode,
    ) -> Result<(), VideoError> {
        if self.metadata.is_none() || size.0 <= 0.0 || size.1 <= 0.0 {
            return Ok(());
        }
        draw_bitmap(dc, &self.bitmap, self.size, size, mode)
    }
}

pub(crate) fn bitmap(
    dc: &ID2D1DeviceContext,
    texture: &ID3D11Texture2D,
) -> Result<ID2D1Bitmap1, VideoError> {
    // SAFETY: The shared/native texture was opened on the D2D context's own drawing device.
    unsafe {
        let surface: IDXGISurface = texture.cast().map_err(error)?;
        dc.CreateBitmapFromDxgiSurface(
            &surface,
            Some(&D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                colorContext: std::mem::ManuallyDrop::new(None),
            }),
        )
        .map_err(error)
    }
}

pub(crate) fn draw_bitmap(
    dc: &ID2D1DeviceContext,
    image: &ID2D1Bitmap1,
    source: (u32, u32),
    target: (f32, f32),
    mode: ResizeMode,
) -> Result<(), VideoError> {
    if !target.0.is_finite()
        || !target.1.is_finite()
        || target.0 <= 0.0
        || target.1 <= 0.0
        || source.0 == 0
        || source.1 == 0
    {
        return Err(error("invalid video drawing bounds"));
    }
    let bounds = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: target.0,
        bottom: target.1,
    };
    let mut destination = bounds;
    if mode != ResizeMode::Stretch {
        let factors = (target.0 / source.0 as f32, target.1 / source.1 as f32);
        let scale = if mode == ResizeMode::Cover {
            factors.0.max(factors.1)
        } else {
            factors.0.min(factors.1)
        };
        let width = source.0 as f32 * scale;
        let height = source.1 as f32 * scale;
        destination = D2D_RECT_F {
            left: (target.0 - width) * 0.5,
            top: (target.1 - height) * 0.5,
            right: (target.0 + width) * 0.5,
            bottom: (target.1 + height) * 0.5,
        };
    }
    // SAFETY: NSH clips this context to the component's bounds. It shares D3D11 command order with raw decoding.
    unsafe {
        let brush = dc
            .CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                None,
            )
            .map_err(error)?;
        dc.FillRectangle(&bounds, &brush);
        dc.DrawBitmap(
            image,
            Some(&destination),
            1.0,
            D2D1_INTERPOLATION_MODE_LINEAR,
            None,
            None,
        );
    }
    Ok(())
}

fn error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::FrameUnavailable, error.to_string())
}
