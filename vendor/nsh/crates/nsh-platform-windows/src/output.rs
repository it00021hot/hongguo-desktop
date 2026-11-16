//! Color precision and display information for native same-canvas composition.

mod color;
mod display;
mod negotiation;
mod probe;
#[cfg(test)]
mod tests;

use std::cell::Cell;

use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL, HWND},
        Graphics::{
            Direct2D::{
                Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT},
                D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
                D2D1_BUFFER_PRECISION_16BPC_FLOAT, ID2D1Bitmap1, ID2D1DeviceContext,
            },
            Direct3D11::{
                D3D11_FORMAT_SUPPORT_BLENDABLE, D3D11_FORMAT_SUPPORT_RENDER_TARGET,
                D3D11_FORMAT_SUPPORT_SHADER_SAMPLE, ID3D11Device,
            },
            Dxgi::{
                Common::{
                    DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709, DXGI_FORMAT,
                    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
                },
                DXGI_ERROR_UNSUPPORTED, DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT,
                IDXGISwapChain1, IDXGISwapChain3,
            },
        },
    },
    core::{Error, Interface, Result},
};

pub use negotiation::{OutputColorFallbackReason, OutputColorMode};

/// The display's CIE 1931 xy primaries and white point, as reported by DXGI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayChromaticities {
    /// Red primary; zero values mean the driver did not report it.
    pub red: [f32; 2],
    /// Green primary.
    pub green: [f32; 2],
    /// Blue primary.
    pub blue: [f32; 2],
    /// White point.
    pub white: [f32; 2],
}

/// Display luminance metadata; unknown or invalid driver values remain `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayLuminance {
    /// Black level.
    pub minimum_nits: Option<f32>,
    /// Peak light output.
    pub peak_nits: Option<f32>,
    /// Peak light output for a full display of white.
    pub full_frame_nits: Option<f32>,
}

/// The display containing the largest part of a window, refreshed on OS changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputColorInfo {
    /// The app's actual composition mode.
    pub mode: OutputColorMode,
    /// An explicit SDR fallback is a mapping target, not measured HDR capability.
    pub fallback_reason: Option<OutputColorFallbackReason>,
    /// The selected display currently uses HDR10 output.
    pub hdr_enabled: bool,
    /// Physical HDR support; `None` when an inactive display exposes only legacy WCG information.
    pub hdr_supported: Option<bool>,
    /// The OS's legacy Advanced Color capability flag, which can include SDR WCG.
    pub advanced_color_supported: Option<bool>,
    /// The OS SDR reference white; unknown HDR white is never fabricated.
    pub sdr_white_level_nits: Option<f32>,
    /// Link/output precision reported by DXGI, distinct from source precision.
    pub bits_per_color: u32,
    /// Display luminance metadata.
    pub luminance: DisplayLuminance,
    /// Display gamut metadata.
    pub chromaticities: DisplayChromaticities,
}

thread_local! {
    static REQUEST: Cell<Option<OutputColorMode>> = const { Cell::new(None) };
    static MODE: Cell<Option<OutputColorMode>> = const { Cell::new(None) };
    static FALLBACK: Cell<Option<OutputColorFallbackReason>> = const { Cell::new(None) };
    static WHITE_NITS: Cell<f32> = const { Cell::new(color::REFERENCE_WHITE_NITS) };
}

pub(crate) fn configure(requested: OutputColorMode) -> Result<()> {
    if requested == OutputColorMode::ScRgb && cfg!(feature = "shader") {
        return Err(Error::new(
            E_NOTIMPL,
            "scRGB cannot use the legacy BGRA8 shader feature",
        ));
    }
    REQUEST.with(|request| {
        if request.get().is_some_and(|existing| existing != requested) {
            return Err(Error::new(
                E_INVALIDARG,
                "all windows on one UI thread must use the same output color mode",
            ));
        }
        request.set(Some(requested));
        if requested != OutputColorMode::Auto {
            MODE.with(|mode| mode.set(Some(requested)));
        }
        Ok(())
    })
}

pub(crate) fn mode() -> OutputColorMode {
    MODE.with(Cell::get).unwrap_or_default()
}

pub(super) fn fallback_reason() -> Option<OutputColorFallbackReason> {
    FALLBACK.with(Cell::get)
}

pub(crate) fn format() -> DXGI_FORMAT {
    match mode() {
        OutputColorMode::Sdr | OutputColorMode::Auto => DXGI_FORMAT_B8G8R8A8_UNORM,
        OutputColorMode::ScRgb => DXGI_FORMAT_R16G16B16A16_FLOAT,
    }
}

pub(crate) fn bytes_per_pixel() -> usize {
    match mode() {
        OutputColorMode::Sdr | OutputColorMode::Auto => 4,
        OutputColorMode::ScRgb => 8,
    }
}

pub(crate) fn info(hwnd: HWND) -> Result<OutputColorInfo> {
    display::get(hwnd)
}

pub(crate) fn invalidate_displays() {
    display::invalidate();
    if REQUEST.get() == Some(OutputColorMode::Auto) {
        let display = display::verify_active_outputs();
        let previous = fallback_reason();
        let display_fallback = matches!(
            previous,
            Some(
                OutputColorFallbackReason::DisplayUnavailable
                    | OutputColorFallbackReason::DisplayColorUnavailable
                    | OutputColorFallbackReason::HdrWhiteUnavailable
            )
        );
        if mode() == OutputColorMode::ScRgb && display.is_err()
            || mode() == OutputColorMode::Sdr && display_fallback && display.is_ok()
        {
            crate::gfx::lost();
        }
    }
}

pub(crate) fn monitor_changed(hwnd: HWND) -> bool {
    mode() == OutputColorMode::ScRgb && display::monitor_changed(hwnd)
}

pub(crate) fn forget_window(hwnd: HWND) {
    display::forget_window(hwnd);
}

pub(crate) fn begin_frame(hwnd: HWND) -> Result<()> {
    if mode() == OutputColorMode::ScRgb {
        let white = info(hwnd)?.sdr_white_level_nits.ok_or_else(|| {
            Error::new(
                DXGI_ERROR_UNSUPPORTED,
                "HDR output did not report its SDR white level",
            )
        })?;
        WHITE_NITS.set(white);
    }
    Ok(())
}

pub(crate) fn paint_key() -> (u64, u32) {
    (crate::gfx::generation(), WHITE_NITS.get().to_bits())
}

pub(crate) fn ui_color(color: [f32; 4]) -> [f32; 4] {
    if mode() == OutputColorMode::ScRgb {
        color::ui_color(color, WHITE_NITS.get())
    } else {
        color
    }
}

pub(crate) fn configure_context(dc: &ID2D1DeviceContext) {
    if mode() == OutputColorMode::ScRgb {
        // SAFETY: the context is live on its owning UI thread.
        unsafe {
            let mut controls = dc.GetRenderingControls();
            controls.bufferPrecision = D2D1_BUFFER_PRECISION_16BPC_FLOAT;
            dc.SetRenderingControls(&controls);
        }
    }
}

pub(crate) fn check_device(device: ID3D11Device) -> Result<ID3D11Device> {
    if REQUEST.get() == Some(OutputColorMode::Auto) {
        if unsafe { device.GetFeatureLevel() }.0
            < windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_0.0
        {
            return Err(Error::new(
                DXGI_ERROR_UNSUPPORTED,
                "automatic native-video composition requires feature level 10; retry with WARP",
            ));
        }
        let (actual, reason) =
            negotiation::choose(cfg!(feature = "shader"), probe::capabilities(&device));
        MODE.set(Some(actual));
        FALLBACK.set(reason);
        display::invalidate();
        if let Some(reason) = reason {
            log::warn!("automatic color negotiation selected SDR: {reason:?}");
        }
    }
    if mode() == OutputColorMode::ScRgb {
        let required = (D3D11_FORMAT_SUPPORT_BLENDABLE.0
            | D3D11_FORMAT_SUPPORT_RENDER_TARGET.0
            | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE.0) as u32;
        // SAFETY: a read-only capability query on this retained device.
        if unsafe { device.CheckFormatSupport(format())? } & required != required {
            return Err(Error::new(
                DXGI_ERROR_UNSUPPORTED,
                "the device cannot compose FP16 scRGB",
            ));
        }
    }
    Ok(device)
}

pub(crate) fn configure_swap_chain(swap: &IDXGISwapChain1) -> Result<()> {
    if mode() == OutputColorMode::ScRgb {
        let swap: IDXGISwapChain3 = swap.cast()?;
        // SAFETY: capability query and explicit linear-scRGB declaration on a live swapchain.
        unsafe {
            let support = swap.CheckColorSpaceSupport(DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)?;
            if support & DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT.0 as u32 == 0 {
                return Err(Error::from_hresult(DXGI_ERROR_UNSUPPORTED));
            }
            swap.SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)?;
        }
    }
    Ok(())
}

pub(crate) fn sdr_bitmap(
    dc: &ID2D1DeviceContext,
    pixels: &[u8],
    width: u32,
    height: u32,
    stride: u32,
) -> Result<ID2D1Bitmap1> {
    sdr_bitmap_with_dpi(dc, pixels, width, height, stride, (96.0, 96.0))
}

pub(crate) fn sdr_bitmap_with_dpi(
    dc: &ID2D1DeviceContext,
    pixels: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    dpi: (f32, f32),
) -> Result<ID2D1Bitmap1> {
    let converted = color::bgra_to_scrgb(pixels, width, height, stride, WHITE_NITS.get())
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let properties = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi.0,
        dpiY: dpi.1,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        colorContext: std::mem::ManuallyDrop::new(None),
    };
    // SAFETY: the owned FP16 RGBA array covers all source rows until CreateBitmap returns.
    unsafe {
        dc.CreateBitmap(
            windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width, height },
            Some(converted.as_ptr().cast()),
            width * 8,
            &properties,
        )
    }
}
