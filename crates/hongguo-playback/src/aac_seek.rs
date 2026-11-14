use hongguo_common::{AppError, AppResult};
use hongguo_video::r#box::parse_boxes;

struct RollRun {
    start: usize,
    end: usize,
    distance: i16,
}
pub(crate) struct AacSeek {
    pub object_type: u8,
    pub codec_history_secs: f64,
    pub bootstrap_samples: usize,
    ps_history_secs: f64,
    rolls: Vec<RollRun>,
}

impl AacSeek {
    pub fn new(asc: &[u8], stbl: &[u8]) -> AppResult<Self> {
        let mut bits = Bits {
            data: asc,
            position: 0,
        };
        let object_type = bits.aot()?;
        let core_rate = bits.frequency()?;
        let channels = bits.read(4)?;
        let mut core_type = object_type;
        let mut output_rate = core_rate;
        let mut sbr = matches!(object_type, 5 | 29).then_some(true);
        if sbr == Some(true) {
            output_rate = bits.frequency()?;
            core_type = bits.aot()?;
        }
        // Unknown predictive/USAC configurations retain the safe source origin;
        // the common LC, HE, HEv2, LD/ELD families have bounded declared overlap.
        let known = matches!(core_type, 2 | 17 | 23 | 39);
        let short = bits.read(1)? != 0;
        if core_type != 39 {
            if bits.read(1)? != 0 {
                bits.read(14)?;
            }
            bits.read(1)?;
        }
        if sbr.is_none() && channels != 0 && bits.remaining() >= 16 {
            let mut extension = bits.clone();
            if extension.read(11)? == 0x2b7 && extension.aot()? == 5 && extension.remaining() > 0 {
                sbr = Some(extension.read(1)? != 0);
            }
        }
        let sbr_frames = if sbr == Some(false) { 1 } else { 2 };
        let frames = if core_type == 39 {
            sbr_frames + 2
        } else {
            sbr_frames
        };
        let mut frame = if short { 960 } else { 1024 };
        if matches!(core_type, 23 | 39) {
            frame /= 2;
        }
        let codec_history_secs = if known {
            f64::from(frame * frames) / f64::from(core_rate)
        } else {
            f64::INFINITY
        };
        let ps_possible = object_type == 29 || (channels <= 1 && sbr != Some(false));
        let ps_history_secs = if ps_possible {
            ps_settling_slots() * 64.0 / f64::from(output_rate)
        } else {
            0.0
        };
        Ok(Self {
            object_type,
            codec_history_secs,
            bootstrap_samples: frames as usize,
            ps_history_secs,
            rolls: roll_groups(stbl)?,
        })
    }

    pub fn history_secs(&self, rate: f64, output_rate: u32) -> f64 {
        if rate == 1.0 {
            return self.codec_history_secs + self.ps_history_secs;
        }
        // FFmpeg n9.0.2 af_atempo.c:268-276 rounds sample_rate/24 up to a power
        // of two; its ring retains 3 windows. Include the history of each chained
        // stage in source time, in addition to AAC's codec pre-roll.
        let window = (output_rate / 24).max(1).next_power_of_two() as f64;
        let stages = if rate > 2.0 { 1.0 + rate.sqrt() } else { 1.0 };
        self.codec_history_secs
            + self.ps_history_secs
            + 3.0 * window / f64::from(output_rate) * stages
    }

    pub fn group_start(&self, target: usize) -> Option<usize> {
        let negative = self
            .rolls
            .iter()
            .filter(|run| run.start <= target && target < run.end && run.distance < 0)
            .map(|run| target.saturating_sub(usize::from(run.distance.unsigned_abs())))
            .min();
        let positive = self
            .rolls
            .iter()
            .filter(|run| run.distance > 0)
            .filter_map(|run| {
                target
                    .checked_sub(run.distance as usize)
                    .map(|latest| latest.min(run.end - 1))
                    .filter(|candidate| *candidate >= run.start)
            })
            .max();
        negative.or(positive)
    }
}

fn ps_settling_slots() -> f64 {
    // The derivation and upstream constants are documented in README.md.
    fn cascade(stages: f64, rho: f64, delay: f64, tolerance: f64) -> f64 {
        let gain = 1.0 + 2.0 * rho;
        let cycles = (tolerance.ln() - (stages * gain.powf(stages)).ln()) / rho.ln();
        stages * delay * cycles.ceil()
    }
    let epsilon = f64::from(f32::EPSILON);
    let allpass = cascade(3.0, 0.651_439_057_531_06, 5.0, epsilon);
    let energy = cascade(3.0, 0.765_928_338_364_65, 1.0, epsilon * epsilon);
    allpass + energy + 14.0 + 12.0 + 20.0
}

#[derive(Clone)]
struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}
impl Bits<'_> {
    fn remaining(&self) -> usize {
        self.data.len() * 8 - self.position
    }
    fn read(&mut self, count: usize) -> AppResult<u32> {
        if count > 32 || count > self.remaining() {
            return Err(invalid("truncated AudioSpecificConfig"));
        }
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | u32::from((self.data[self.position / 8] >> (7 - self.position % 8)) & 1);
            self.position += 1;
        }
        Ok(value)
    }
    fn aot(&mut self) -> AppResult<u8> {
        let value = self.read(5)?;
        Ok(if value == 31 {
            32 + self.read(6)?
        } else {
            value
        } as u8)
    }
    fn frequency(&mut self) -> AppResult<u32> {
        const RATES: [u32; 13] = [
            96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
        ];
        let index = self.read(4)?;
        let rate = if index == 15 {
            self.read(24)?
        } else {
            *RATES
                .get(index as usize)
                .ok_or_else(|| invalid("invalid AAC frequency index"))?
        };
        if rate == 0 {
            return Err(invalid("zero AAC frequency"));
        }
        Ok(rate)
    }
}

fn roll_groups(data: &[u8]) -> AppResult<Vec<RollRun>> {
    let atoms = parse_boxes(data, 0, data.len());
    if atoms.iter().map(|atom| atom.total_size).sum::<usize>() != data.len() {
        return Err(invalid("truncated audio sample table"));
    }
    let mut output = Vec::new();
    for kind in [b"roll", b"prol"] {
        let relevant = |payload: &[u8]| payload.get(4..8) == Some(kind);
        let description = atoms
            .iter()
            .find(|atom| atom.is("sgpd") && relevant(&data[atom.start..atom.start + atom.size]));
        let groups = atoms
            .iter()
            .find(|atom| atom.is("sbgp") && relevant(&data[atom.start..atom.start + atom.size]));
        let Some(description) = description else {
            if groups.is_some() {
                return Err(invalid("audio group has no description"));
            }
            continue;
        };
        let description = &data[description.start..description.start + description.size];
        let version = description[0];
        if version > 2 {
            return Err(invalid("unsupported sgpd version"));
        }
        let mut at = 8;
        let default_length = if version >= 1 {
            word(description, &mut at)? as usize
        } else {
            2
        };
        let default_index = if version >= 2 {
            word(description, &mut at)?
        } else {
            0
        };
        let count = word(description, &mut at)? as usize;
        if count > (description.len() - at) / 2 {
            return Err(invalid("invalid roll description count"));
        }
        let mut distances = Vec::with_capacity(count);
        for _ in 0..count {
            let size = if version >= 1 && default_length == 0 {
                word(description, &mut at)? as usize
            } else {
                default_length
            };
            let end = at
                .checked_add(size)
                .filter(|end| *end <= description.len())
                .ok_or_else(|| invalid("truncated roll description"))?;
            if size != 2 {
                return Err(invalid("invalid roll description size"));
            }
            distances.push(i16::from_be_bytes([description[at], description[at + 1]]));
            at = end;
        }
        let distance = |index: u32| -> AppResult<Option<i16>> {
            let index = if index == 0 { default_index } else { index };
            if index == 0 {
                return Ok(None);
            }
            distances
                .get(index as usize - 1)
                .copied()
                .map(Some)
                .ok_or_else(|| invalid("invalid audio roll group index"))
        };
        let Some(groups) = groups else {
            if let Some(distance) = distance(0)? {
                output.push(RollRun {
                    start: 0,
                    end: usize::MAX,
                    distance,
                });
            }
            continue;
        };
        let groups = &data[groups.start..groups.start + groups.size];
        if groups[0] > 1 {
            return Err(invalid("unsupported sbgp version"));
        }
        let mut at = if groups[0] == 1 { 12 } else { 8 };
        let count = word(groups, &mut at)? as usize;
        if count > (groups.len() - at) / 8 {
            return Err(invalid("invalid audio group run count"));
        }
        let mut start = 0usize;
        for _ in 0..count {
            let count = word(groups, &mut at)? as usize;
            let index = word(groups, &mut at)?;
            let end = start
                .checked_add(count)
                .ok_or_else(|| invalid("audio roll sample index overflow"))?;
            if count > 0
                && let Some(distance) = distance(index)?
            {
                output.push(RollRun {
                    start,
                    end,
                    distance,
                });
            }
            start = end;
        }
    }
    Ok(output)
}
fn word(data: &[u8], at: &mut usize) -> AppResult<u32> {
    let bytes = data
        .get(*at..at.saturating_add(4))
        .ok_or_else(|| invalid("truncated audio roll group"))?;
    *at += 4;
    Ok(u32::from_be_bytes(bytes.try_into().expect("four bytes")))
}
fn invalid(message: &str) -> AppError {
    AppError::Media(message.into())
}

#[cfg(test)]
#[path = "aac_seek_tests.rs"]
mod tests;
