use objc2::encode::{Encode, Encoding};

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Time {
    value: i64,
    scale: i32,
    flags: u32,
    epoch: i64,
}

unsafe impl Encode for Time {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING],
    );
}

impl Time {
    pub const ZERO: Self = Self {
        value: 0,
        scale: 1,
        flags: 1,
        epoch: 0,
    };

    pub fn seconds(seconds: f64) -> Self {
        Self {
            value: (seconds * 90_000.0).round() as i64,
            scale: 90_000,
            flags: 1,
            epoch: 0,
        }
    }

    pub fn value(self) -> Option<f64> {
        (self.flags & 1 != 0 && self.flags & 0x1c == 0 && self.scale > 0)
            .then(|| self.value as f64 / f64::from(self.scale))
    }
}
