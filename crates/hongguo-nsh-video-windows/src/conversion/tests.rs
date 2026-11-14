//! Windows GPU regression tests: no window, media URL or external decoder is used.
use super::*;
use hongguo_media_types::{ChromaSampling, CropRect, NativeFrame, NativePlane};
use nsh_platform_windows::{DisplayChromaticities, DisplayLuminance, OutputColorMode};
use std::sync::Arc;
use windows::{
    Win32::Graphics::{
        Direct2D::{
            D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory,
            ID2D1Factory1,
        },
        Direct3D::D3D_DRIVER_TYPE_WARP,
        Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_USAGE_STAGING, D3D11CreateDevice,
        },
        Dxgi::{Common::DXGI_FORMAT_R16G16B16A16_FLOAT, IDXGIDevice},
    },
    core::Interface,
};

#[test]
fn original_ten_bit_samples_reach_fp16_without_an_eight_bit_intermediate() {
    let (device, dc) = device();
    let mut conversion = Conversion::new(&device).expect("compile the actual HLSL shaders");
    let source = frame();
    let output = target(&device);
    let mut rtv = None;
    unsafe { device.CreateRenderTargetView(&output, None, Some(&mut rtv)) }.unwrap();
    conversion
        .render(
            &dc,
            &FrameImage::Cpu(Arc::new(source)),
            &rtv.unwrap(),
            None,
            (4, 4),
            display(),
        )
        .unwrap();
    unsafe { dc.EndDraw(None, None) }.unwrap();
    let rgba = read(&device, &output);
    let left = rgba[0][0];
    let right = rgba[1][0];
    // A BGRA8 intermediate would make these adjacent ten-bit codes identical.
    assert!(
        right > left,
        "distinct 10-bit luma collapsed: {left:#06x} {right:#06x}"
    );
    assert_eq!(rgba[0][3], 0x3c00);
    for pixel in &rgba {
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
    }
}

#[test]
fn malformed_cpu_chroma_layout_is_rejected_before_gpu_access() {
    let (device, dc) = device();
    let mut conversion = Conversion::new(&device).unwrap();
    let mut source = frame();
    source.planes[2].width = 1;
    let output = target(&device);
    let mut rtv = None;
    unsafe { device.CreateRenderTargetView(&output, None, Some(&mut rtv)) }.unwrap();
    let error = conversion
        .render(
            &dc,
            &FrameImage::Cpu(Arc::new(source)),
            &rtv.unwrap(),
            None,
            (4, 4),
            display(),
        )
        .unwrap_err();
    unsafe { dc.EndDraw(None, None) }.unwrap();
    assert_eq!(
        error.code(),
        hongguo_nsh_video_api::ErrorCode::FrameFormatUnsupported
    );
}

#[test]
fn feature_level_ten_can_present_original_precision_with_shader_model_four() {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_0;
    let (device, dc) = device_at(Some(&[D3D_FEATURE_LEVEL_10_0]));
    assert_eq!(unsafe { device.GetFeatureLevel() }, D3D_FEATURE_LEVEL_10_0);
    let mut conversion =
        Conversion::new(&device).expect("compile SM4 rather than requiring SM5 on an FL10 device");
    let texture = target(&device);
    let mut view = None;
    unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut view)) }.unwrap();
    conversion
        .render(
            &dc,
            &FrameImage::Cpu(Arc::new(frame())),
            &view.unwrap(),
            None,
            (4, 4),
            display(),
        )
        .unwrap();
    unsafe { dc.EndDraw(None, None) }.unwrap();
    let rgba = read(&device, &texture);
    assert!(rgba[1][0] > rgba[0][0]);
}

#[test]
fn hdr_working_mrt_precedes_final_display_mapping() {
    let (device, dc) = device();
    let mut conversion = Conversion::new(&device).unwrap();
    let mut source = frame();
    source.color.primaries = Some(9);
    source.color.matrix = Some(9);
    source.color.transfer = Some(16);
    source.planes[0].data = vec![1023_u16; 16]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>()
        .into();
    let mapped = target(&device);
    let working = target(&device);
    let (mut mapped_view, mut working_view) = (None, None);
    unsafe {
        device
            .CreateRenderTargetView(&mapped, None, Some(&mut mapped_view))
            .unwrap();
        device
            .CreateRenderTargetView(&working, None, Some(&mut working_view))
            .unwrap();
    }
    let mut output = display();
    output.hdr_enabled = true;
    output.sdr_white_level_nits = Some(200.0);
    output.luminance.peak_nits = Some(1000.0);
    conversion
        .render(
            &dc,
            &FrameImage::Cpu(Arc::new(source)),
            &mapped_view.unwrap(),
            Some(&working_view.unwrap()),
            (4, 4),
            output,
        )
        .unwrap();
    unsafe { dc.EndDraw(None, None) }.unwrap();
    let mapped = read(&device, &mapped);
    let working = read(&device, &working);
    // Enhancement receives the unmapped source (10,000 / 80 = 125 scRGB),
    // while the same-canvas raw fallback stays within the measured display peak.
    assert!(working[0][0] > mapped[0][0]);
    assert!(working[0][0] > 0x5600); // >96, well above the 1000-nit target
    assert!(mapped[0][0] <= 0x4a40); // <=12.5 (1000/80)
}

fn frame() -> NativeFrame {
    let plane = |width: usize, height: usize, values: Vec<u16>| NativePlane {
        data: values
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
            .into(),
        width,
        height,
        stride: width * 2,
        bytes_per_sample: 2,
    };
    let color = ColorMetadata {
        primaries: Some(1),
        transfer: Some(8),
        matrix: Some(1),
        full_range: Some(true),
        ..Default::default()
    };
    NativeFrame {
        width: 4,
        height: 4,
        bit_depth: 10,
        chroma: ChromaSampling::Yuv420,
        planes: vec![
            plane(
                4,
                4,
                (0..16)
                    .map(|i| if i % 4 == 0 { 512 } else { 513 })
                    .collect(),
            ),
            plane(2, 2, vec![512; 4]),
            plane(2, 2, vec![512; 4]),
        ],
        pts: Some(0.0),
        color,
        crop: CropRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        rotation_degrees: 0,
    }
}

fn display() -> OutputColorInfo {
    OutputColorInfo {
        mode: OutputColorMode::ScRgb,
        fallback_reason: None,
        hdr_enabled: false,
        hdr_supported: Some(false),
        advanced_color_supported: Some(false),
        sdr_white_level_nits: Some(80.0),
        bits_per_color: 8,
        luminance: DisplayLuminance {
            minimum_nits: None,
            peak_nits: Some(80.0),
            full_frame_nits: Some(80.0),
        },
        chromaticities: DisplayChromaticities {
            red: [0.64, 0.33],
            green: [0.30, 0.60],
            blue: [0.15, 0.06],
            white: [0.3127, 0.3290],
        },
    }
}

fn device() -> (ID3D11Device, ID2D1DeviceContext) {
    device_at(None)
}

fn device_at(
    levels: Option<&[windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL]>,
) -> (ID3D11Device, ID2D1DeviceContext) {
    // SAFETY: these tests own their offscreen WARP device and its D2D context.
    unsafe {
        let mut device = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            levels,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .unwrap();
        let device = device.unwrap();
        let factory: ID2D1Factory1 =
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap();
        let dxgi: IDXGIDevice = device.cast().unwrap();
        let dc = factory
            .CreateDevice(&dxgi)
            .unwrap()
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
            .unwrap();
        use windows::Win32::Graphics::Direct2D::{
            Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT},
            D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
        };
        let scratch = dc
            .CreateBitmap(
                D2D_SIZE_U {
                    width: 4,
                    height: 4,
                },
                None,
                0,
                &D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                    ..Default::default()
                },
            )
            .unwrap();
        dc.SetTarget(&scratch);
        dc.BeginDraw();
        (device, dc)
    }
}

fn target(device: &ID3D11Device) -> ID3D11Texture2D {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 4,
        Height: 4,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: windows::Win32::Graphics::Direct3D11::D3D11_BIND_RENDER_TARGET.0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.unwrap();
    texture.unwrap()
}

fn read(device: &ID3D11Device, texture: &ID3D11Texture2D) -> Vec<[u16; 4]> {
    // SAFETY: staging Map blocks until this test's preceding GPU commands finish;
    // every row is read within its reported pitch and is unmapped before release.
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        device
            .CreateTexture2D(&desc, None, Some(&mut staging))
            .unwrap();
        let staging = staging.unwrap();
        let context = device.GetImmediateContext().unwrap();
        context.CopyResource(&staging, texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .unwrap();
        let mut result = Vec::new();
        for y in 0..desc.Height as usize {
            let row = std::slice::from_raw_parts(
                mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                desc.Width as usize * 8,
            );
            for pixel in row.as_chunks::<8>().0 {
                result.push(std::array::from_fn(|c| {
                    u16::from_le_bytes([pixel[c * 2], pixel[c * 2 + 1]])
                }));
            }
        }
        context.Unmap(&staging, 0);
        result
    }
}
