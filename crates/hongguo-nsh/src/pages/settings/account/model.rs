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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resend_deadlines_belong_to_the_phone_and_survive_switching_and_reopening() {
        let mut deadlines = CodeDeadlines::default();
        deadlines.record("13800138000".into(), 0, 10_000);
        assert_eq!(deadlines.remaining("13800138000", 10_001), 60);
        assert_eq!(deadlines.remaining("13900139000", 10_001), 0);
        deadlines.record("13900139000".into(), 30, 20_000);
        let reopened = deadlines.clone();
        assert_eq!(reopened.remaining("13800138000", 40_500), 30);
        assert_eq!(reopened.remaining("13900139000", 40_500), 10);
        assert_eq!(reopened.remaining("13800138000", 70_001), 0);
    }

    #[test]
    fn numeric_fields_are_bounded_and_submission_requires_a_valid_complete_form() {
        assert_eq!(digits("a138-0013 800012", MOBILE_LENGTH), "13800138000");
        assert_eq!(digits("1a2-3４45678", CODE_LENGTH), "123456");
        assert!(!can_login("", "", false));
        assert!(!can_login("13800138000", "123", false));
        assert!(!can_login("13800138000", "1234", true));
        assert!(!can_login("13800138000", "1234567", false));
        assert!(!can_login("13800138000", "123a", false));
        assert!(can_login("13800138000", "1234", false));
        assert!(can_login("13800138000", "123456", false));
    }
}
