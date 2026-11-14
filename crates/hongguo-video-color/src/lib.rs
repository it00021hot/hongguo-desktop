mod gamut;
mod transfer;

use hongguo_media_types::ColorMetadata;

pub const LUT_SIZE: usize = 4096;
pub const LUT_WIDTH: u32 = 64;
pub const LUT_HEIGHT: u32 = 64;
pub const SCRGB_WHITE_NITS: f32 = 80.0;
pub const WGSL: &str = include_str!("../shaders/color.wgsl");
pub const HLSL: &str = include_str!("../shaders/color.hlsl");
const MAX_SIGNAL_NITS: f64 = 10_000.0;
const HLG_REFERENCE_PEAK: f64 = 1000.0;
const HLG_REFERENCE_GAMMA: f64 = 1.2;
const LOG_SIGNAL_MAX: f64 = 13.287856641840545;
type Matrix = [[f64; 3]; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Primaries {
    pub red: [f64; 2],
    pub green: [f64; 2],
    pub blue: [f64; 2],
    pub white: [f64; 2],
}

impl Primaries {
    pub const BT709: Self = Self {
        red: [0.64, 0.33],
        green: [0.30, 0.60],
        blue: [0.15, 0.06],
        white: [0.3127, 0.3290],
    };
    pub const BT2020: Self = Self {
        red: [0.708, 0.292],
        green: [0.170, 0.797],
        blue: [0.131, 0.046],
        white: [0.3127, 0.3290],
    };
    pub const P3_D65: Self = Self {
        red: [0.680, 0.320],
        green: [0.265, 0.690],
        blue: [0.150, 0.060],
        white: [0.3127, 0.3290],
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputEncoding {
    LinearScRgb,
    Srgb,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayTarget {
    pub primaries: Primaries,
    pub sdr_white_nits: f32,
    pub peak_nits: f32,
    pub encoding: OutputEncoding,
    pub gamut_mapping: bool,
}

impl DisplayTarget {
    pub fn sdr() -> Self {
        Self {
            primaries: Primaries::BT709,
            sdr_white_nits: 100.0,
            peak_nits: 100.0,
            encoding: OutputEncoding::Srgb,
            gamut_mapping: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ColorParams {
    pub range: [f32; 4],
    pub yuv: [[f32; 4]; 3],
    pub source_to_display: [[f32; 4]; 3],
    pub display_to_output: [[f32; 4]; 3],
    pub output_to_display: [[f32; 4]; 3],
    pub display: [f32; 4],
    pub source_luma: [f32; 4],
    pub transfer: [f32; 4],
}

impl ColorParams {
    pub fn bytes(&self) -> Vec<u8> {
        self.range
            .iter()
            .chain(self.yuv.iter().flatten())
            .chain(self.source_to_display.iter().flatten())
            .chain(self.display_to_output.iter().flatten())
            .chain(self.output_to_display.iter().flatten())
            .chain(self.display.iter())
            .chain(self.source_luma.iter())
            .chain(self.transfer.iter())
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct ColorTransform {
    pub params: ColorParams,
    pub lut: Vec<[f32; 4]>,
    pub assumed_metadata: bool,
}

impl ColorTransform {
    pub fn new(
        color: &ColorMetadata,
        bit_depth: u8,
        height: usize,
        target: DisplayTarget,
    ) -> Result<Self, String> {
        if !(8..=16).contains(&bit_depth) {
            return Err("unsupported sample precision".into());
        }
        if !target.sdr_white_nits.is_finite()
            || !target.peak_nits.is_finite()
            || target.sdr_white_nits <= 0.0
            || target.peak_nits < target.sdr_white_nits
        {
            return Err("invalid measured display luminance".into());
        }
        let matrix = color.matrix.unwrap_or(if height >= 720 { 1 } else { 6 });
        let (kr, kb) = match matrix {
            1 => (0.2126, 0.0722),
            5 | 6 => (0.299, 0.114),
            7 => (0.212, 0.087),
            9 => (0.2627, 0.0593),
            _ => return Err(format!("unsupported CICP YUV matrix {matrix}")),
        };
        let kg = 1.0 - kr - kb;
        let yuv = [
            [1.0, 0.0, 2.0 * (1.0 - kr)],
            [
                1.0,
                -2.0 * kb * (1.0 - kb) / kg,
                -2.0 * kr * (1.0 - kr) / kg,
            ],
            [1.0, 2.0 * (1.0 - kb), 0.0],
        ];
        let inferred_primaries = match matrix {
            9 => 9,
            5 => 5,
            6 | 7 => 6,
            _ => 1,
        };
        let source_primaries = gamut::from_cicp(color.primaries.unwrap_or(inferred_primaries))?;
        let source_to_display = gamut::conversion(source_primaries, target.primaries)?;
        let display_to_output = gamut::conversion(target.primaries, Primaries::BT709)?;
        let output_to_display = gamut::conversion(Primaries::BT709, target.primaries)?;
        let transfer = color.transfer.unwrap_or(1);
        let hdr = transfer == 16 || transfer == 18;
        let source_peak = if hdr {
            color
                .content_light
                .filter(|light| light.max_content_light_level > 0)
                .map(|light| f64::from(light.max_content_light_level))
                .or_else(|| {
                    color
                        .mastering_display
                        .filter(|master| master.max_luminance > 0)
                        .map(|master| f64::from(master.max_luminance) / 10_000.0)
                })
                .unwrap_or(if transfer == 16 {
                    MAX_SIGNAL_NITS
                } else {
                    HLG_REFERENCE_PEAK
                })
        } else {
            f64::from(target.sdr_white_nits)
        };
        let source_peak =
            source_peak
                .max(f64::from(target.sdr_white_nits))
                .min(if transfer == 18 {
                    HLG_REFERENCE_PEAK
                } else {
                    MAX_SIGNAL_NITS
                });
        let luma = gamut::luminance(source_primaries)?;
        let output_peak = f64::from(target.peak_nits);
        let display_luma = gamut::luminance(target.primaries)?;
        let lut = (0..LUT_SIZE)
            .map(|index| {
                let signal = index as f64 / (LUT_SIZE - 1) as f64;
                let nits = (signal * LOG_SIGNAL_MAX).exp2() - 1.0;
                let linear = transfer::eotf(signal, transfer, f64::from(target.sdr_white_nits))?;
                let mapped = if hdr {
                    transfer::bt2390(nits, source_peak, output_peak)
                } else {
                    nits
                };
                let encoded = transfer::srgb_oetf(nits / f64::from(target.sdr_white_nits));
                Ok([linear as f32, mapped as f32, encoded as f32, 0.0])
            })
            .collect::<Result<Vec<_>, String>>()?;
        let maximum = ((1_u32 << bit_depth) - 1) as f32;
        let scale = (1_u32 << (bit_depth - 8)) as f32;
        let range = if color.full_range == Some(true) {
            [0.0, maximum, (1_u32 << (bit_depth - 1)) as f32, maximum]
        } else {
            [16.0 * scale, 219.0 * scale, 128.0 * scale, 224.0 * scale]
        };
        Ok(Self {
            params: ColorParams {
                range,
                yuv: rows(yuv),
                source_to_display: rows(source_to_display),
                display_to_output: rows(display_to_output),
                output_to_display: rows(output_to_display),
                display: [
                    display_luma[0] as f32,
                    display_luma[1] as f32,
                    if target.encoding == OutputEncoding::LinearScRgb {
                        1.0
                    } else {
                        0.0
                    },
                    SCRGB_WHITE_NITS,
                ],
                source_luma: [
                    luma[0] as f32,
                    luma[1] as f32,
                    luma[2] as f32,
                    HLG_REFERENCE_GAMMA as f32,
                ],
                transfer: [
                    if transfer == 18 { 1.0 } else { 0.0 },
                    HLG_REFERENCE_PEAK as f32,
                    1.0 / LOG_SIGNAL_MAX as f32,
                    if target.gamut_mapping { 1.0 } else { 0.0 },
                ],
            },
            lut,
            assumed_metadata: color.primaries.is_none()
                || color.transfer.is_none()
                || color.matrix.is_none()
                || color.full_range.is_none(),
        })
    }

    pub fn lut_bytes(&self) -> Vec<u8> {
        self.lut
            .iter()
            .flatten()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    pub fn transform(&self, code_values: [f32; 3]) -> [f32; 3] {
        let range = self.params.range;
        let yuv = [
            (code_values[0] - range[0]) / range[1],
            (code_values[1] - range[2]) / range[3],
            (code_values[2] - range[2]) / range[3],
        ];
        let signal = multiply(self.params.yuv, yuv);
        let mut linear = signal.map(|value| self.lookup(value, 0));
        if self.params.transfer[0] != 0.0 {
            let p = self.params.source_luma;
            let scene_luminance = (linear[0] * p[0] + linear[1] * p[1] + linear[2] * p[2]).max(0.0);
            let ootf = scene_luminance.powf(p[3] - 1.0) * self.params.transfer[1];
            linear = linear.map(|value| value * ootf);
        }
        let mut display = multiply(self.params.source_to_display, linear);
        let peak = display.into_iter().fold(0_f32, f32::max);
        let mapped = self.lookup(log_coordinate(peak), 1);
        if peak > 0.0 {
            display = display.map(|value| value * mapped / peak);
        }
        let luminance = display[0] * self.params.display[0]
            + display[1] * self.params.display[1]
            + display[2] * (1.0 - self.params.display[0] - self.params.display[1]);
        let lower = display.into_iter().fold(0_f32, f32::min);
        if lower < 0.0 && luminance > 0.0 && self.params.transfer[3] != 0.0 {
            let compression = luminance / (luminance - lower);
            display = display.map(|value| luminance + (value - luminance) * compression);
        }
        let output = multiply(self.params.display_to_output, display);
        if self.params.display[2] != 0.0 {
            output.map(|value| value / SCRGB_WHITE_NITS)
        } else {
            output.map(|value| self.lookup(log_coordinate(value), 2))
        }
    }

    fn lookup(&self, coordinate: f32, channel: usize) -> f32 {
        let index = coordinate.clamp(0.0, 1.0) * (LUT_SIZE - 1) as f32;
        let lower = index as usize;
        let upper = (lower + 1).min(LUT_SIZE - 1);
        self.lut[lower][channel]
            + (self.lut[upper][channel] - self.lut[lower][channel]) * index.fract()
    }
}

fn log_coordinate(nits: f32) -> f32 {
    (1.0 + nits.max(0.0)).log2() / LOG_SIGNAL_MAX as f32
}
fn multiply(matrix: [[f32; 4]; 3], value: [f32; 3]) -> [f32; 3] {
    matrix.map(|row| row[0] * value[0] + row[1] * value[1] + row[2] * value[2])
}
fn rows(matrix: Matrix) -> [[f32; 4]; 3] {
    matrix.map(|row| [row[0] as f32, row[1] as f32, row[2] as f32, 0.0])
}

#[cfg(test)]
mod tests;
