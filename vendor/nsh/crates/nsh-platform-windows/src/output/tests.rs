use super::*;

#[test]
fn output_mode_cannot_mix_sdr_and_scrgb_windows_on_one_ui_thread() {
    std::thread::spawn(|| {
        configure(OutputColorMode::Sdr).unwrap();
        assert_eq!(format(), DXGI_FORMAT_B8G8R8A8_UNORM);
        assert!(configure(OutputColorMode::Sdr).is_ok());
        assert!(configure(OutputColorMode::ScRgb).is_err());
    })
    .join()
    .unwrap();
}

#[test]
#[cfg(feature = "shader")]
fn legacy_bgra_shader_feature_cannot_silently_truncate_scrgb() {
    std::thread::spawn(|| {
        assert_eq!(
            configure(OutputColorMode::ScRgb).unwrap_err().code(),
            E_NOTIMPL
        );
    })
    .join()
    .unwrap();
}

#[test]
#[cfg(not(feature = "shader"))]
fn fp16_color_and_alpha_survive_an_offscreen_target_before_final_composition() {
    use windows::Win32::Graphics::Direct2D::{
        Common::{D2D_SIZE_U, D2D1_COLOR_F},
        D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ,
        D2D1_INTERPOLATION_MODE_LINEAR, D2D1_MAP_OPTIONS_READ,
    };

    std::thread::spawn(|| {
        configure(OutputColorMode::ScRgb).unwrap();
        let gfx = crate::gfx::get().unwrap();
        let source = crate::render::target_bitmap(&gfx.dc, (2, 2), 1.0).unwrap();
        let target = crate::render::target_bitmap(&gfx.dc, (2, 2), 1.0).unwrap();
        let mut properties = crate::gfx::target_properties(1.0, true);
        properties.bitmapOptions = D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW;
        // SAFETY: every resource belongs to this thread's live device; draws and maps are paired.
        unsafe {
            let read = gfx
                .dc
                .CreateBitmap(
                    D2D_SIZE_U {
                        width: 2,
                        height: 2,
                    },
                    None,
                    0,
                    &properties,
                )
                .unwrap();
            gfx.dc.SetTarget(&source);
            gfx.dc.BeginDraw();
            gfx.dc.Clear(Some(&D2D1_COLOR_F {
                r: 4.0,
                g: 2.0,
                b: 1.0,
                a: 1.0,
            }));
            gfx.dc.EndDraw(None, None).unwrap();
            gfx.dc.SetTarget(&target);
            gfx.dc.BeginDraw();
            gfx.dc.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            gfx.dc.DrawBitmap(
                &source,
                None,
                0.5,
                D2D1_INTERPOLATION_MODE_LINEAR,
                None,
                None,
            );
            gfx.dc.EndDraw(None, None).unwrap();
            gfx.dc.SetTarget(None);
            read.CopyFromBitmap(None, &target, None).unwrap();
            let mapped = read.Map(D2D1_MAP_OPTIONS_READ).unwrap();
            let bytes = std::slice::from_raw_parts(mapped.bits, 8);
            let rgba = std::array::from_fn::<_, 4, _>(|channel| {
                u16::from_ne_bytes([bytes[channel * 2], bytes[channel * 2 + 1]])
            });
            read.Unmap().unwrap();
            assert_eq!(rgba, [0x4000, 0x3c00, 0x3800, 0x3c00]);
        }
    })
    .join()
    .unwrap();
}
