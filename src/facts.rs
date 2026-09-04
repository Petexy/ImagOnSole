//! Turning what the filesystem knows into what a person reads.
//!
//! Three small conversions, kept together because each of them is the sort
//! that is written slightly differently in two places and then disagrees:
//! how large a file is, when it was written, and how big a picture is.

use std::time::SystemTime;

/// A file's length, in the units somebody would say it in.
///
/// Powers of two under names of powers of ten, which is what every file
/// manager on this machine does; being right about it here and different from
/// everything else beside it would be its own kind of wrong.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// How many pixels across and down, with the millions of them beside it.
pub fn pixels(width: u32, height: u32) -> String {
    let millions = (f64::from(width) * f64::from(height)) / 1_000_000.0;
    if millions >= 0.95 {
        format!("{width} × {height}  ·  {millions:.1} MP")
    } else {
        format!("{width} × {height}")
    }
}

/// When a file was last written, as a date and a time.
///
/// In UTC, deliberately and only because the alternative is reading the zone
/// database, and a viewer that shipped its own half-right idea of local time
/// would be wrong in a way nobody could see. The date is what is being asked
/// for here — which of two holidays this is from — and that is the same date
/// either way for all but a few hours of it.
pub fn when(time: SystemTime) -> String {
    let Ok(since) = time.duration_since(SystemTime::UNIX_EPOCH) else {
        return String::from("Before 1970");
    };
    let seconds = since.as_secs();
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (year, month, day) = civil(days);
    format!(
        "{day} {} {year}, {:02}:{:02}",
        MONTHS[(month - 1) as usize],
        rest / 3600,
        (rest % 3600) / 60
    )
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Days since 1970 to a calendar date.
///
/// Howard Hinnant's `civil_from_days`, which is the standard way of doing this
/// without a table: shift the era so that March is the first month and the
/// leap day falls at the end of the cycle, where it stops being a special
/// case.
fn civil(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = (days - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted + 2) / 5 + 1) as u32;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sizes_read_the_way_a_file_manager_says_them() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1024), "1.0 KB");
        assert_eq!(size(1024 * 1024 * 2 + 512 * 1024), "2.5 MB");
        assert_eq!(size(1024 * 1024 * 100), "100 MB");
    }

    #[test]
    fn the_epoch_is_the_first_of_january() {
        assert_eq!(when(SystemTime::UNIX_EPOCH), "1 January 1970, 00:00");
    }

    #[test]
    fn a_leap_day_is_a_leap_day() {
        // 2024-02-29T12:00:00Z
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_709_208_000);
        assert_eq!(when(time), "29 February 2024, 12:00");
    }

    #[test]
    fn a_century_that_is_not_a_leap_year() {
        // 1900-03-01 is day -25508 from the epoch; 1900 was not a leap year.
        assert_eq!(civil(-25_508), (1900, 3, 1));
        assert_eq!(civil(-25_509), (1900, 2, 28));
    }

    #[test]
    fn megapixels_appear_only_when_there_is_a_megapixel() {
        assert!(pixels(4000, 3000).contains("12.0 MP"));
        assert_eq!(pixels(64, 64), "64 × 64");
    }
}
