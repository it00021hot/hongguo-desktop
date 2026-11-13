use hongguo_common::{
    consts::units::{MS_PER_DAY, MS_PER_HOUR, MS_PER_SEC},
    utils::time::beijing_date,
};

pub(super) fn date_label(timestamp_ms: i64, chinese: bool) -> String {
    let date = beijing_date(timestamp_ms);
    if date.len() != 8 {
        return String::new();
    }
    let month = date[4..6].trim_start_matches('0');
    let day = date[6..8].trim_start_matches('0');
    if chinese {
        format!("{month}月{day}日")
    } else {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let month = date[4..6].parse::<usize>().expect("numeric Beijing month");
        format!("{} {day}", MONTHS[month - 1])
    }
}

pub(super) fn publish_time(timestamp_ms: i64) -> String {
    const BEIJING_OFFSET: i64 = 8 * MS_PER_HOUR;
    const MS_PER_MINUTE: i64 = 60 * MS_PER_SEC;
    let date = beijing_date(timestamp_ms);
    if date.len() != 8 {
        return String::new();
    }
    let daytime = (timestamp_ms + BEIJING_OFFSET).rem_euclid(MS_PER_DAY);
    let hour = daytime / MS_PER_HOUR;
    let minute = daytime.rem_euclid(MS_PER_HOUR) / MS_PER_MINUTE;
    format!("{}-{} {hour:02}:{minute:02}", &date[4..6], &date[6..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_badge_uses_the_short_month_name_in_english() {
        assert_eq!(date_label(1_794_384_000_000, false), "Nov 11");
        assert_eq!(date_label(1_794_384_000_000, true), "11月11日");
    }

    #[test]
    fn reservation_publish_time_uses_beijing_minutes_and_the_corresponding_date() {
        assert_eq!(publish_time(1_794_384_000_000), "11-11 16:00");
        assert_eq!(publish_time(1_794_416_400_000), "11-12 01:00");
        assert_eq!(publish_time(0), "");
    }
}
