use std::{cell::RefCell, collections::HashMap};

use windows::{
    Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
            DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO,
            DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
            GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
        },
        Foundation::{ERROR_INSUFFICIENT_BUFFER, HWND},
        Graphics::{
            Dxgi::{
                Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, CreateDXGIFactory1,
                DXGI_OUTPUT_DESC1, IDXGIFactory1, IDXGIOutput6,
            },
            Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromWindow},
        },
    },
    core::{Error, Interface, Result},
};

use super::{
    DisplayChromaticities, DisplayLuminance, OutputColorInfo, color::REFERENCE_WHITE_NITS,
};

const ADVANCED_COLOR_SUPPORTED: u32 = 1;
const DISPLAY_CONFIG_ATTEMPTS: usize = 3;

thread_local! {
    static DISPLAYS: RefCell<HashMap<isize, OutputColorInfo>> = RefCell::default();
    static WINDOWS: RefCell<HashMap<isize, isize>> = RefCell::default();
}

pub(super) fn invalidate() {
    DISPLAYS.with_borrow_mut(HashMap::clear);
}

pub(super) fn get(hwnd: HWND) -> Result<OutputColorInfo> {
    // SAFETY: a live window handle; the nearest monitor is the window's output.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let key = monitor.0 as isize;
    if let Some(info) = DISPLAYS.with_borrow(|displays| displays.get(&key).copied()) {
        return Ok(info);
    }
    let info = match query(monitor) {
        Ok(info) => info,
        Err(error) if super::mode() == super::OutputColorMode::Sdr => {
            log::warn!(
                "display color measurements unavailable; using the standard SDR mapping target: {error}"
            );
            conservative_sdr()
        }
        Err(error) => return Err(error),
    };
    DISPLAYS.with_borrow_mut(|displays| displays.insert(key, info));
    Ok(info)
}

pub(super) fn monitor_changed(hwnd: HWND) -> bool {
    // SAFETY: a message from this live native window identifies its nearest output.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }.0 as isize;
    WINDOWS.with_borrow_mut(|windows| windows.insert(hwnd.0 as isize, monitor) != Some(monitor))
}

pub(super) fn forget_window(hwnd: HWND) {
    WINDOWS.with_borrow_mut(|windows| windows.remove(&(hwnd.0 as isize)));
}

fn query(monitor: HMONITOR) -> Result<OutputColorInfo> {
    // SAFETY: factory/output queries return owned COM interfaces and descriptors.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut adapter_index = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_index) } {
        let mut output_index = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(output_index) } {
            let desc = unsafe { output.GetDesc()? };
            if desc.Monitor == monitor {
                let output: IDXGIOutput6 = output.cast()?;
                return describe(unsafe { output.GetDesc1()? });
            }
            output_index += 1;
        }
        adapter_index += 1;
    }
    Err(Error::from_hresult(
        windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND,
    ))
}

fn describe(desc: DXGI_OUTPUT_DESC1) -> Result<OutputColorInfo> {
    let hdr_enabled = desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
    let target = target_for(&desc.DeviceName)?;
    let (advanced_color_supported, sdr_white_level_nits) = if let Some(path) = target {
        let mut advanced = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                size: size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        // SAFETY: the header identifies the containing SDK-sized request structure.
        let supported = (unsafe { DisplayConfigGetDeviceInfo(&mut advanced.header) } == 0)
            .then(|| unsafe { advanced.Anonymous.value } & ADVANCED_COLOR_SUPPORTED != 0);
        let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                size: size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        let white_nits = if hdr_enabled {
            (unsafe { DisplayConfigGetDeviceInfo(&mut white.header) } == 0)
                .then(|| white.SDRWhiteLevel as f32 / 1000.0 * REFERENCE_WHITE_NITS)
                .filter(|white| white.is_finite() && *white > 0.0)
        } else {
            Some(REFERENCE_WHITE_NITS)
        };
        (supported, white_nits)
    } else {
        (None, (!hdr_enabled).then_some(REFERENCE_WHITE_NITS))
    };
    let hdr_supported = if hdr_enabled {
        Some(true)
    } else if advanced_color_supported == Some(false) {
        Some(false)
    } else {
        None
    };
    Ok(OutputColorInfo {
        mode: super::mode(),
        fallback_reason: super::fallback_reason(),
        hdr_enabled,
        hdr_supported,
        advanced_color_supported,
        sdr_white_level_nits,
        bits_per_color: desc.BitsPerColor,
        luminance: DisplayLuminance {
            minimum_nits: valid_luminance(desc.MinLuminance, true),
            peak_nits: valid_luminance(desc.MaxLuminance, false),
            full_frame_nits: valid_luminance(desc.MaxFullFrameLuminance, false),
        },
        chromaticities: DisplayChromaticities {
            red: desc.RedPrimary,
            green: desc.GreenPrimary,
            blue: desc.BluePrimary,
            white: desc.WhitePoint,
        },
    })
}

pub(super) fn verify_active_outputs() -> std::result::Result<(), super::OutputColorFallbackReason> {
    use super::OutputColorFallbackReason as Reason;
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|_| Reason::DisplayUnavailable)?;
    let mut found = false;
    let mut adapter_index = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_index) } {
        let mut output_index = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(output_index) } {
            let basic = unsafe { output.GetDesc() }.map_err(|_| Reason::DisplayColorUnavailable)?;
            if basic.AttachedToDesktop.as_bool() {
                found = true;
                let output: IDXGIOutput6 =
                    output.cast().map_err(|_| Reason::DisplayColorUnavailable)?;
                let info = describe(
                    unsafe { output.GetDesc1() }.map_err(|_| Reason::DisplayColorUnavailable)?,
                )
                .map_err(|_| Reason::DisplayColorUnavailable)?;
                if info.hdr_enabled && info.sdr_white_level_nits.is_none() {
                    return Err(Reason::HdrWhiteUnavailable);
                }
            }
            output_index += 1;
        }
        adapter_index += 1;
    }
    if found {
        Ok(())
    } else {
        Err(Reason::DisplayUnavailable)
    }
}

fn conservative_sdr() -> OutputColorInfo {
    OutputColorInfo {
        mode: super::mode(),
        fallback_reason: super::fallback_reason(),
        hdr_enabled: false,
        hdr_supported: None,
        advanced_color_supported: None,
        sdr_white_level_nits: None,
        bits_per_color: 0,
        luminance: DisplayLuminance {
            minimum_nits: None,
            peak_nits: None,
            full_frame_nits: None,
        },
        chromaticities: DisplayChromaticities {
            red: [0.0; 2],
            green: [0.0; 2],
            blue: [0.0; 2],
            white: [0.0; 2],
        },
    }
}

fn valid_luminance(value: f32, zero_valid: bool) -> Option<f32> {
    (value.is_finite()
        && if zero_valid {
            value >= 0.0
        } else {
            value > 0.0
        })
    .then_some(value)
}

fn target_for(name: &[u16; 32]) -> Result<Option<DISPLAYCONFIG_PATH_INFO>> {
    for _ in 0..DISPLAY_CONFIG_ATTEMPTS {
        let (mut paths_count, mut modes_count) = (0, 0);
        // SAFETY: counts are valid out pointers for active-path enumeration.
        unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_count, &mut modes_count)
        }
        .ok()?;
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_count as usize];
        let status = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut paths_count,
                paths.as_mut_ptr(),
                &mut modes_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        status.ok()?;
        for path in paths.into_iter().take(paths_count as usize) {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                ..Default::default()
            };
            if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } == 0
                && nul_prefix(&source.viewGdiDeviceName) == nul_prefix(name)
            {
                return Ok(Some(path));
            }
        }
        return Ok(None);
    }
    Err(Error::from_hresult(HRESULT_INSUFFICIENT_BUFFER))
}

const HRESULT_INSUFFICIENT_BUFFER: windows::core::HRESULT =
    windows::core::HRESULT(0x8007007a_u32 as i32);

fn nul_prefix(text: &[u16]) -> &[u16] {
    &text[..text
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(text.len())]
}
