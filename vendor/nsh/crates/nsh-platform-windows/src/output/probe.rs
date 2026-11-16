use super::OutputColorFallbackReason as Reason;
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_FORMAT_SUPPORT_BLENDABLE, D3D11_FORMAT_SUPPORT_RENDER_TARGET,
            D3D11_FORMAT_SUPPORT_SHADER_SAMPLE, ID3D11Device,
        },
        Dxgi::{
            Common::{
                DXGI_ALPHA_MODE_IGNORE, DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709,
                DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
            },
            CreateDXGIFactory1, DXGI_SCALING_STRETCH,
            DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT, DXGI_SWAP_CHAIN_DESC1,
            DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2,
            IDXGISwapChain3,
        },
    },
    core::Interface,
};

pub(super) fn capabilities(device: &ID3D11Device) -> Result<(), Reason> {
    super::display::verify_active_outputs()?;
    let required = (D3D11_FORMAT_SUPPORT_BLENDABLE.0
        | D3D11_FORMAT_SUPPORT_RENDER_TARGET.0
        | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE.0) as u32;
    let support = unsafe { device.CheckFormatSupport(DXGI_FORMAT_R16G16B16A16_FLOAT) }
        .map_err(|_| Reason::Fp16Unavailable)?;
    if support & required != required {
        return Err(Reason::Fp16Unavailable);
    }
    // A one-pixel offscreen swap chain tests the actual OS/device path. It has no
    // HWND or visual, never presents, and cannot cover the application's UI.
    let present = || -> windows::core::Result<()> {
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory1()?;
            let swap = factory.CreateSwapChainForComposition(
                device,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: 1,
                    Height: 1,
                    Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    ..Default::default()
                },
                None,
            )?;
            let swap: IDXGISwapChain3 = swap.cast()?;
            if swap.CheckColorSpaceSupport(DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)?
                & DXGI_SWAP_CHAIN_COLOR_SPACE_SUPPORT_FLAG_PRESENT.0 as u32
                == 0
            {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Graphics::Dxgi::DXGI_ERROR_UNSUPPORTED,
                ));
            }
            swap.SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709)
        }
    };
    present().map_err(|_| Reason::ScRgbPresentationUnavailable)
}
