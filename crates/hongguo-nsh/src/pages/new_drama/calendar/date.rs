use chrono::{Datelike, Local, NaiveDate, TimeZone};

pub(super) fn weekday_key(date: &str, today: &str) -> Option<&'static str> {
    let parsed = NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
    if date == today {
        return Some("newDrama.today");
    }
    Some(
        [
            "newDrama.weekday.0",
            "newDrama.weekday.1",
            "newDrama.weekday.2",
            "newDrama.weekday.3",
            "newDrama.weekday.4",
            "newDrama.weekday.5",
            "newDrama.weekday.6",
        ][parsed.weekday().num_days_from_sunday() as usize],
    )
}

pub(super) fn day_month(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y%m%d").map_or_else(
        |_| date.to_owned(),
        |date| date.format("%-m/%-d").to_string(),
    )
}

pub(super) fn today() -> String {
    Local::now().format("%Y%m%d").to_string()
}

pub(super) fn publish_time(timestamp_ms: i64) -> String {
    format_publish(timestamp_ms, &Local)
}

fn format_publish<Tz: TimeZone>(timestamp_ms: i64, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if timestamp_ms <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_else(String::new, |date| {
        date.with_timezone(zone).format("%m-%d %H:%M").to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_and_compact_dates_use_the_server_date_without_inventing_invalid_days() {
        assert_eq!(weekday_key("20261111", "20261111"), Some("newDrama.today"));
        assert_eq!(
            weekday_key("20261108", "20261111"),
            Some("newDrama.weekday.0")
        );
        assert_eq!(
            weekday_key("20261114", "20261111"),
            Some("newDrama.weekday.6")
        );
        assert_eq!(day_month("20261108"), "11/8");
        assert_eq!(weekday_key("20260230", "20261111"), None);
        assert_eq!(day_month("bad"), "bad");
    }

    #[test]
    fn publish_time_uses_the_supplied_zone_and_omits_unscheduled_entries() {
        let east = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let west = chrono::FixedOffset::west_opt(8 * 3600).unwrap();
        assert_eq!(format_publish(1_794_384_000_000, &east), "11-11 16:00");
        assert_eq!(format_publish(1_794_384_000_000, &west), "11-11 00:00");
        assert_eq!(format_publish(0, &east), "");
    }
}
