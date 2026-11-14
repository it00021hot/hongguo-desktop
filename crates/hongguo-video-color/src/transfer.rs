const PQ_M1: f64 = 2610.0 / 16384.0;
const PQ_M2: f64 = 2523.0 / 32.0;
const PQ_C1: f64 = 3424.0 / 4096.0;
const PQ_C2: f64 = 2413.0 / 128.0;
const PQ_C3: f64 = 2392.0 / 128.0;
const PQ_PEAK: f64 = 10_000.0;
const HLG_A: f64 = 0.17883277;
const HLG_B: f64 = 0.28466892;
const HLG_C: f64 = 0.55991073;

fn pq_decode(signal: f64) -> f64 {
    let power = signal.max(0.0).powf(1.0 / PQ_M2);
    ((power - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * power)).powf(1.0 / PQ_M1) * PQ_PEAK
}

fn pq_encode(nits: f64) -> f64 {
    let power = (nits.max(0.0) / PQ_PEAK).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * power) / (1.0 + PQ_C3 * power)).powf(PQ_M2)
}

pub fn eotf(signal: f64, transfer: u8, white: f64) -> Result<f64, String> {
    let linear = match transfer {
        1 | 6 | 14 | 15 => signal.powf(2.4) * white,
        4 => signal.powf(2.2) * white,
        5 => signal.powf(2.8) * white,
        7 => {
            (if signal < 0.0912 {
                signal / 4.0
            } else {
                ((signal + 0.1115) / 1.1115).powf(1.0 / 0.45)
            }) * white
        }
        8 => signal * white,
        13 => {
            (if signal <= 0.04045 {
                signal / 12.92
            } else {
                ((signal + 0.055) / 1.055).powf(2.4)
            }) * white
        }
        16 => pq_decode(signal),
        18 => {
            if signal <= 0.5 {
                signal * signal / 3.0
            } else {
                (((signal - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
            }
        }
        _ => return Err(format!("unsupported CICP transfer {transfer}")),
    };
    Ok(linear)
}

pub fn srgb_oetf(linear: f64) -> f64 {
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

pub fn bt2390(nits: f64, source_peak: f64, target_peak: f64) -> f64 {
    if source_peak <= target_peak {
        return nits;
    }
    let source_pq = pq_encode(source_peak);
    let target = pq_encode(target_peak) / source_pq;
    let knee = (1.5 * target - 0.5).max(0.0);
    let value = (pq_encode(nits) / source_pq).min(1.0);
    let mapped = if value <= knee {
        value
    } else {
        let t = (value - knee) / (1.0 - knee);
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * knee
            + (t3 - 2.0 * t2 + t) * (1.0 - knee)
            + (-2.0 * t3 + 3.0 * t2) * target
    };
    pq_decode(mapped * source_pq)
}
