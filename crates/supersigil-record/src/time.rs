//! UTC times in whole seconds, written in a fixed-width RFC 3339 form.
//!
//! The civil-date conversion is Howard Hinnant's `days_from_civil` and
//! `civil_from_days` over the proleptic Gregorian calendar.

use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How long an unused review basis or a replaced document is kept: 14
/// days, git's default prune age.
pub const GRACE_SECONDS: i64 = 14 * DAY;

/// How long a removal stays published before the removed file is deleted.
pub const REMOVAL_DELAY_SECONDS: i64 = DAY;

const DAY: i64 = 86_400;

/// First second the fixed-width form can write: `0000-01-01T00:00:00Z`.
const MIN_UNIX: i64 = -62_167_219_200;

/// Last second the fixed-width form can write: `9999-12-31T23:59:59Z`.
const MAX_UNIX: i64 = 253_402_300_799;

/// A UTC time in whole seconds since the Unix epoch.
///
/// Written as `YYYY-MM-DDTHH:MM:SSZ`, which sorts like the times it names.
/// Serialized as that string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UtcTime(i64);

impl UtcTime {
    /// Reads the system clock, dropping fractions of a second.
    #[must_use]
    pub fn now() -> Self {
        let unix = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |s| -s),
        };
        Self::from_unix(unix)
    }

    /// The time `unix` seconds after the epoch (before it when negative).
    ///
    /// Times outside the years 0000 to 9999, which the fixed-width form
    /// cannot write, are clamped to the nearest one it can.
    #[must_use]
    pub const fn from_unix(unix: i64) -> Self {
        Self(if unix < MIN_UNIX {
            MIN_UNIX
        } else if unix > MAX_UNIX {
            MAX_UNIX
        } else {
            unix
        })
    }

    /// Seconds since the Unix epoch, negative before it.
    #[must_use]
    pub const fn unix(self) -> i64 {
        self.0
    }

    /// The UTC date, `YYYY-MM-DD`.
    #[must_use]
    pub fn date(self) -> String {
        let (year, month, day) = civil_from_days(self.0.div_euclid(DAY));
        format!("{year:04}-{month:02}-{day:02}")
    }
}

impl fmt::Display for UtcTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.0.rem_euclid(DAY);
        write!(
            f,
            "{}T{:02}:{:02}:{:02}Z",
            self.date(),
            seconds / 3_600,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}

/// A string that is not a time in the form `YYYY-MM-DDTHH:MM:SSZ`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a UTC time of the form YYYY-MM-DDTHH:MM:SSZ")]
pub struct ParseTimeError(String);

impl FromStr for UtcTime {
    type Err = ParseTimeError;

    /// Accepts exactly the strings [`fmt::Display`] writes: a field out of
    /// range, such as 30 February or hour 24, changes the written form and
    /// is refused.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || ParseTimeError(text.to_owned());
        let bytes = text.as_bytes();
        if bytes.len() != 20 {
            return Err(invalid());
        }
        let field = |range: std::ops::Range<usize>| -> Option<i64> {
            bytes[range].iter().try_fold(0, |value, &b| {
                b.is_ascii_digit().then(|| value * 10 + i64::from(b - b'0'))
            })
        };
        let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
            field(0..4),
            field(5..7),
            field(8..10),
            field(11..13),
            field(14..16),
            field(17..19),
        ) else {
            return Err(invalid());
        };
        let days = days_from_civil(year, month, day);
        let time = Self::from_unix(days * DAY + hour * 3_600 + minute * 60 + second);
        if time.to_string() == text {
            Ok(time)
        } else {
            Err(invalid())
        }
    }
}

impl Serialize for UtcTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UtcTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date. Months outside
/// 1 to 12 and days outside the month give some other date.
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_from_march = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian `(year, month, day)` of a day count since
/// 1970-01-01.
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Times and their fixed-width form, across the epoch, leap days, and
    /// century years.
    const KNOWN: [(i64, &str); 8] = [
        (0, "1970-01-01T00:00:00Z"),
        (-1, "1969-12-31T23:59:59Z"),
        (1_791_894_896, "2026-10-13T12:34:56Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (1_709_208_000, "2024-02-29T12:00:00Z"),
        (-2_203_891_201, "1900-02-28T23:59:59Z"),
        (4_107_542_399, "2100-02-28T23:59:59Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
    ];

    #[test]
    fn known_times_format_and_parse() {
        for (unix, text) in KNOWN {
            let time = UtcTime::from_unix(unix);
            assert_eq!(time.unix(), unix);
            assert_eq!(time.to_string(), text);
            assert_eq!(text.parse::<UtcTime>().unwrap(), time, "{text}");
            assert_eq!(time.date(), text[..10], "{text}");
        }
    }

    #[test]
    fn a_century_year_is_a_leap_year_only_every_400_years() {
        // 1900 and 2100 have no 29 February; 2000 has one.
        assert_eq!(
            UtcTime::from_unix(-2_203_891_201 + 1).to_string(),
            "1900-03-01T00:00:00Z"
        );
        assert_eq!(
            UtcTime::from_unix(4_107_542_399 + 1).to_string(),
            "2100-03-01T00:00:00Z"
        );
        assert_eq!(
            UtcTime::from_unix(951_782_400 - 1).to_string(),
            "2000-02-28T23:59:59Z"
        );
    }

    #[test]
    fn times_from_year_0_to_3000_round_trip_and_sort_as_text() {
        let mut unix = UtcTime::from_unix(i64::MIN).unix();
        let mut previous = String::new();
        while unix <= 32_503_680_000 {
            let text = UtcTime::from_unix(unix).to_string();
            assert_eq!(text.parse::<UtcTime>().unwrap().unix(), unix, "{text}");
            assert!(previous < text, "{previous} then {text}");
            previous = text;
            unix += 86_400 * 7 + 3_661;
        }
    }

    #[test]
    fn times_outside_the_four_digit_years_are_clamped() {
        assert_eq!(
            UtcTime::from_unix(i64::MAX).to_string(),
            "9999-12-31T23:59:59Z"
        );
        assert_eq!(
            UtcTime::from_unix(i64::MIN).to_string(),
            "0000-01-01T00:00:00Z"
        );
        assert_eq!(UtcTime::from_unix(i64::MIN).unix(), -62_167_219_200);
    }

    #[test]
    fn only_the_fixed_width_form_parses() {
        for text in [
            "",
            "2026-10-13",
            "2026-10-13T12:34:56",
            "2026-10-13T12:34:56.000Z",
            "2026-10-13 12:34:56Z",
            "2026-10-13t12:34:56z",
            "2026-10-13T12:34:56+00:00",
            "+026-10-13T12:34:56Z",
            "2026-1-13T12:34:56Z ",
            "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-00-10T00:00:00Z",
            "2026-13-10T00:00:00Z",
            "2026-10-00T00:00:00Z",
            "2026-10-13T24:00:00Z",
            "2026-10-13T12:60:00Z",
            "2026-10-13T12:34:60Z",
            "２026-10-13T12:34:56Z",
        ] {
            let error = text.parse::<UtcTime>().unwrap_err();
            assert!(
                error.to_string().contains("YYYY-MM-DDTHH:MM:SSZ"),
                "{error}"
            );
        }
    }

    #[test]
    fn serde_uses_the_fixed_width_string() {
        let time = UtcTime::from_unix(1_791_894_896);
        let json = serde_json::to_string(&time).unwrap();
        assert_eq!(json, "\"2026-10-13T12:34:56Z\"");
        assert_eq!(serde_json::from_str::<UtcTime>(&json).unwrap(), time);
        serde_json::from_str::<UtcTime>("\"2026-10-13\"").unwrap_err();
        serde_json::from_str::<UtcTime>("1791894896").unwrap_err();
    }

    #[test]
    fn times_order_by_instant() {
        let earlier = UtcTime::from_unix(-5);
        let later = UtcTime::from_unix(1_791_894_896);
        assert!(earlier < later);
        assert!(later < UtcTime::from_unix(1_791_894_897));
    }

    /// The system time `time` names.
    fn system_time(time: UtcTime) -> SystemTime {
        let seconds = std::time::Duration::from_secs(time.unix().unsigned_abs());
        if time.unix() < 0 {
            UNIX_EPOCH - seconds
        } else {
            UNIX_EPOCH + seconds
        }
    }

    #[test]
    fn now_is_the_system_clock_without_its_fraction_of_a_second() {
        // Compared only with readings of the clock taken around it, so the
        // test holds whatever date the clock is set to.
        let second = std::time::Duration::from_secs(1);
        let before = SystemTime::now();
        let now = system_time(UtcTime::now());
        let after = SystemTime::now();
        assert!(
            before < now + second && now < after + second,
            "{before:?} {now:?} {after:?}"
        );
    }

    #[test]
    fn grace_periods_are_whole_days() {
        assert_eq!(GRACE_SECONDS, 14 * 86_400);
        assert_eq!(REMOVAL_DELAY_SECONDS, 86_400);
    }
}
