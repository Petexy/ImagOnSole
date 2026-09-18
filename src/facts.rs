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
        lxb_toolkit::i18n::decimal(format!("{value:.1} {}", UNITS[unit]))
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// How many pixels across and down, with the millions of them beside it.
pub fn pixels(width: u32, height: u32) -> String {
    let millions = (f64::from(width) * f64::from(height)) / 1_000_000.0;
    if millions >= 0.95 {
        lxb_toolkit::i18n::decimal(format!("{width} × {height}  ·  {millions:.1} MP"))
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
        return String::from(crate::i18n::text("before-1970"));
    };
    let seconds = since.as_secs();
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (year, month, day) = civil(days);
    crate::message!("file-date", "day" => day.to_string(),
        "month" => lxb_toolkit::i18n::month(month as usize), "year" => year.to_string(),
        // The clock the session is set to, which is a setting rather than a
        // language: Settings > System > Clock, read out of the shell's own
        // file. See `lxb_toolkit::settings::twelve_hour_clock`.
        "time" => lxb_toolkit::i18n::time_of_day((rest / 3600) as u32, ((rest % 3600) / 60) as u32))
}

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

    /// The figures the lines below are written with, carrying whatever
    /// decimal mark the session's language uses — a comma in Polish. Asked
    /// for the same way `size` asks for it, because what this test is about
    /// is the rounding and the units, and a run under a Polish session that
    /// failed on a full stop would be failing about the wrong thing.
    fn as_written(number: &str) -> String {
        lxb_toolkit::i18n::decimal(number.to_string())
    }

    /// The same for a date, which is a sentence the catalog builds.
    fn date(day: u32, month: usize, year: i64, time: &str) -> String {
        crate::message!("file-date", "day" => day.to_string(),
            "month" => lxb_toolkit::i18n::month(month), "year" => year.to_string(),
            "time" => time.to_string())
    }

    #[test]
    fn sizes_read_the_way_a_file_manager_says_them() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1024), as_written("1.0 KB"));
        assert_eq!(size(1024 * 1024 * 2 + 512 * 1024), as_written("2.5 MB"));
        assert_eq!(size(1024 * 1024 * 100), "100 MB");
    }

    /// And what the two shipped languages make of the same number, which is
    /// the half the line above cannot state.
    #[test]
    fn a_quantity_carries_the_decimal_mark_of_the_language() {
        assert_eq!(
            lxb_toolkit::i18n::decimal("2.5 MB".to_string()),
            // The two Englishes, Hindi and Chinese keep the full stop; the
            // six others write a comma. It follows the language and not the
            // country — see `lxb_toolkit::i18n::decimal`.
            match lxb_toolkit::i18n::language() {
                "en-GB" | "en-US" | "hi" | "zh-CN" => "2.5 MB",
                _ => "2,5 MB",
            }
        );
    }

    /// Midnight, and it writes the hour the way the shell's own clock does —
    /// without a leading zero. One `clock-24-hour` in the toolkit's catalogs
    /// answers for the corner of the start screen, the guide and this line
    /// alike, so there is one place to change it and no way for the three to
    /// drift apart.
    #[test]
    fn the_epoch_is_the_first_of_january() {
        assert_eq!(when(SystemTime::UNIX_EPOCH), date(1, 1, 1970, "0:00"));
    }

    #[test]
    fn a_leap_day_is_a_leap_day() {
        // 2024-02-29T12:00:00Z
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_709_208_000);
        assert_eq!(when(time), date(29, 2, 2024, "12:00"));
    }

    /// The date each language writes, named rather than taken off this
    /// machine: Polish puts the month in the genitive.
    #[test]
    fn a_date_is_written_the_way_each_language_writes_one() {
        let catalog = crate::i18n::Catalog::new(crate::i18n::RESOURCES);
        let written = |locale: &str| {
            let mut args = crate::i18n::FluentArgs::new();
            args.set("day", "29");
            args.set(
                "month",
                lxb_toolkit::i18n::Catalog::new(lxb_toolkit::i18n::RESOURCES)
                    .text_for(locale, "month-february")
                    .to_string(),
            );
            args.set("year", "2024");
            args.set("time", "12:00");
            catalog.format_for(locale, "file-date", &args)
        };
        assert_eq!(written("en-GB"), "29 February 2024, 12:00");
        assert_eq!(written("pl"), "29 lutego 2024, 12:00");
        // America puts the month first, which is the whole of `en-US.ftl`.
        assert_eq!(written("en-US"), "February 29, 2024, 12:00");
        // And an English that is neither reads out of the British catalog.
        assert_eq!(written("en_AU.UTF-8"), "29 February 2024, 12:00");
        assert_eq!(written("fr"), "29 février 2024, 12:00");
        // Spanish fences the month with *de* on both sides, which a separator
        // and three values could not have written; Portuguese does the same.
        assert_eq!(written("es"), "29 de febrero de 2024, 12:00");
        assert_eq!(written("pt_BR"), "29 de fevereiro de 2024, 12:00");
        // German and Russian point the day, and Russian's month is in the
        // genitive as Polish's is.
        assert_eq!(written("de_AT.UTF-8"), "29. Februar 2024, 12:00");
        assert_eq!(written("ru"), "29 февраля 2024 г., 12:00");
        assert_eq!(written("hi"), "29 फ़रवरी 2024, 12:00");
        // Chinese goes largest first, and the month's name is its number and
        // a character — so the whole date is one message here as well.
        assert_eq!(written("zh_CN.UTF-8"), "2024年2月29日 12:00");
    }

    #[test]
    fn a_century_that_is_not_a_leap_year() {
        // 1900-03-01 is day -25508 from the epoch; 1900 was not a leap year.
        assert_eq!(civil(-25_508), (1900, 3, 1));
        assert_eq!(civil(-25_509), (1900, 2, 28));
    }

    #[test]
    fn megapixels_appear_only_when_there_is_a_megapixel() {
        assert!(pixels(4000, 3000).contains(&as_written("12.0 MP")));
        assert_eq!(pixels(64, 64), "64 × 64");
    }
}
