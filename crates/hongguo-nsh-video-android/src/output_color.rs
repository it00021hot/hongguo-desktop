use hongguo_video_color::{DisplayTarget, OutputEncoding, Primaries};

pub(crate) fn target(floating: bool, hdr_peak: f32) -> DisplayTarget {
    // 80 is a normalization reference, not a measurement of this display's SDR white.
    let hdr = floating && hdr_peak > 0.;
    DisplayTarget {
        primaries: if hdr {
            Primaries::BT2020
        } else {
            Primaries::BT709
        },
        sdr_white_nits: 80.,
        peak_nits: if hdr { hdr_peak } else { 80. },
        encoding: if floating {
            OutputEncoding::LinearScRgb
        } else {
            OutputEncoding::Srgb
        },
        gamut_mapping: !hdr,
    }
}
