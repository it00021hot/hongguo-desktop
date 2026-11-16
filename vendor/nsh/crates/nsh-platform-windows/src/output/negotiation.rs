/// The pixel and color space used by all native windows on this UI thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputColorMode {
    /// Negotiate verified scRGB; retain a standard SDR canvas when unavailable.
    Auto,
    /// The original BGRA8 SDR composition pipeline.
    #[default]
    Sdr,
    /// FP16 linear scRGB, including offscreen composition targets.
    /// The legacy BGRA8 shader feature must be disabled.
    ScRgb,
}

/// Why automatic negotiation chose the standard SDR composition pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputColorFallbackReason {
    /// The upstream shader feature only accepts BGRA8.
    LegacyShader,
    /// No active DXGI output (including remote or headless sessions).
    DisplayUnavailable,
    /// The OS or driver cannot report the active output's color state.
    DisplayColorUnavailable,
    /// HDR is active but its SDR reference white is unknown.
    HdrWhiteUnavailable,
    /// The device cannot sample, blend and render FP16.
    Fp16Unavailable,
    /// The OS or driver cannot present an scRGB FP16 swap chain.
    ScRgbPresentationUnavailable,
}

pub(super) fn choose(
    legacy_shader: bool,
    capabilities: Result<(), OutputColorFallbackReason>,
) -> (OutputColorMode, Option<OutputColorFallbackReason>) {
    match capabilities {
        _ if legacy_shader => (
            OutputColorMode::Sdr,
            Some(OutputColorFallbackReason::LegacyShader),
        ),
        Ok(()) => (OutputColorMode::ScRgb, None),
        Err(reason) => (OutputColorMode::Sdr, Some(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use OutputColorFallbackReason as Reason;
    #[test]
    fn remote_session_without_dxgi_outputs_keeps_a_working_sdr_canvas() {
        assert_eq!(
            choose(false, Err(Reason::DisplayUnavailable)),
            (OutputColorMode::Sdr, Some(Reason::DisplayUnavailable))
        );
    }
    #[test]
    fn old_driver_without_output6_or_fp16_falls_back_with_the_actual_reason() {
        for reason in [
            Reason::DisplayColorUnavailable,
            Reason::HdrWhiteUnavailable,
            Reason::Fp16Unavailable,
            Reason::ScRgbPresentationUnavailable,
        ] {
            assert_eq!(
                choose(false, Err(reason)),
                (OutputColorMode::Sdr, Some(reason))
            );
        }
        assert_eq!(choose(false, Ok(())), (OutputColorMode::ScRgb, None));
        assert_eq!(
            choose(true, Ok(())),
            (OutputColorMode::Sdr, Some(Reason::LegacyShader))
        );
        assert_ne!(OutputColorMode::Auto, OutputColorMode::default());
    }
}
