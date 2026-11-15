use std::collections::BTreeMap;

pub(super) const MOBILE_LENGTH: usize = 11;
pub(super) const CODE_LENGTH: usize = 6;
const RETRY_DEFAULT_SECONDS: u32 = 60;
const MILLIS_PER_SECOND: i64 = 1_000;

#[derive(Clone, Default, PartialEq)]
pub(super) struct CodeDeadlines(BTreeMap<String, i64>);

impl CodeDeadlines {
    pub(super) fn record(&mut self, mobile: String, retry_seconds: u32, now: i64) {
        self.0.retain(|_, deadline| *deadline > now);
        let seconds = if retry_seconds == 0 {
            RETRY_DEFAULT_SECONDS
        } else {
            retry_seconds
        };
        self.0.insert(
            mobile,
            now.saturating_add(i64::from(seconds) * MILLIS_PER_SECOND),
        );
    }

    pub(super) fn remaining(&self, mobile: &str, now: i64) -> u32 {
        let remaining = self
            .0
            .get(mobile)
            .copied()
            .unwrap_or(now)
            .saturating_sub(now)
            .max(0);
        ((remaining.saturating_add(MILLIS_PER_SECOND - 1) / MILLIS_PER_SECOND)
            .min(i64::from(u32::MAX))) as u32
    }
}

pub(super) fn digits(input: &str, limit: usize) -> String {
    input
        .chars()
        .filter(char::is_ascii_digit)
        .take(limit)
        .collect()
}

pub(super) fn valid_mobile(mobile: &str) -> bool {
    mobile.len() == MOBILE_LENGTH && mobile.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn can_login(mobile: &str, code: &str, busy: bool) -> bool {
    !busy
        && valid_mobile(mobile)
        && (4..=CODE_LENGTH).contains(&code.len())
        && code.bytes().all(|byte| byte.is_ascii_digit())
}
