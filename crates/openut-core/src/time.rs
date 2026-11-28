//! UTC timestamps and local calendar values for log entries.

use std::fmt;

const MILLIS_PER_SECOND: i64 = 1_000;
const MILLIS_PER_MINUTE: i64 = 60 * MILLIS_PER_SECOND;
const MILLIS_PER_HOUR: i64 = 60 * MILLIS_PER_MINUTE;
const MILLIS_PER_DAY: i64 = 24 * MILLIS_PER_HOUR;
const DAYS_PER_ERA: i64 = 146_097;
const DAYS_BEFORE_UNIX_EPOCH: i64 = 719_468;

/// An instant, in milliseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogTime(pub i64);

/// Minutes east of UTC, as written in the log header (e.g. "+9" -> 540).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcOffset(pub i16);

/// A local calendar date in the proleptic Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

/// A local time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalClock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millisecond: u16,
}

impl LogTime {
    /// Interprets validated local date and clock fields at the given UTC offset.
    pub fn from_local(
        date: LogDate,
        hour: u8,
        minute: u8,
        second: u8,
        millisecond: u16,
        offset: UtcOffset,
    ) -> Option<Self> {
        if hour > 23 || minute > 59 || second > 59 || millisecond > 999 {
            return None;
        }

        let days = i128::from(date.days_since_epoch()?);
        let millis = i64::from(hour) * MILLIS_PER_HOUR
            + i64::from(minute) * MILLIS_PER_MINUTE
            + i64::from(second) * MILLIS_PER_SECOND
            + i64::from(millisecond)
            - i64::from(offset.0) * MILLIS_PER_MINUTE;
        let utc_millis = days * i128::from(MILLIS_PER_DAY) + i128::from(millis);
        i64::try_from(utc_millis).ok().map(Self)
    }

    /// Returns the calendar date and clock at the given UTC offset.
    pub fn to_local(self, offset: UtcOffset) -> (LogDate, LocalClock) {
        let shifted_millis =
            self.0.rem_euclid(MILLIS_PER_DAY) + i64::from(offset.0) * MILLIS_PER_MINUTE;
        let local_days =
            self.0.div_euclid(MILLIS_PER_DAY) + shifted_millis.div_euclid(MILLIS_PER_DAY);
        let millis = shifted_millis.rem_euclid(MILLIS_PER_DAY);
        let (year, month, day) = calendar_date(local_days + DAYS_BEFORE_UNIX_EPOCH);
        (
            LogDate {
                // An i64 count of milliseconds spans fewer than 293 million years.
                year: year as i32,
                month,
                day,
            },
            LocalClock {
                hour: (millis / MILLIS_PER_HOUR) as u8,
                minute: (millis / MILLIS_PER_MINUTE % 60) as u8,
                second: (millis / MILLIS_PER_SECOND % 60) as u8,
                millisecond: (millis % MILLIS_PER_SECOND) as u16,
            },
        )
    }
}

impl LogDate {
    /// Returns the number of days since 1970-01-01, or `None` for an invalid date.
    pub fn days_since_epoch(self) -> Option<i64> {
        let leap = self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0);
        let month_length = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        if self.day == 0 || self.day > month_length {
            return None;
        }

        // Count from March so that leap days fall at the end of their year.
        let jan_or_feb = self.month <= 2;
        let march_year = i64::from(self.year) - i64::from(jan_or_feb);
        let march_month = if jan_or_feb {
            i64::from(self.month) + 9
        } else {
            i64::from(self.month) - 3
        };
        let era = march_year.div_euclid(400);
        let year_of_era = march_year.rem_euclid(400);
        let day_of_year = (153 * march_month + 2) / 5 + i64::from(self.day) - 1;
        let days = era * DAYS_PER_ERA + 365 * year_of_era + year_of_era / 4 - year_of_era / 100
            + day_of_year;
        Some(days - DAYS_BEFORE_UNIX_EPOCH)
    }

    /// Returns the date for a day count since 1970-01-01 if its year fits in i32.
    pub fn from_days_since_epoch(days: i64) -> Option<Self> {
        let (year, month, day) = calendar_date(days.checked_add(DAYS_BEFORE_UNIX_EPOCH)?);
        Some(Self {
            year: i32::try_from(year).ok()?,
            month,
            day,
        })
    }
}

// Converts a day count from March 1 of year 0 in the Gregorian calendar.
fn calendar_date(days: i64) -> (i64, u8, u8) {
    let era = days.div_euclid(DAYS_PER_ERA);
    let day_of_era = days.rem_euclid(DAYS_PER_ERA);
    // The final day in an era is a leap day of its fourth century.
    let century = (day_of_era / 36_524).min(3);
    let day_of_century = day_of_era - century * 36_524;
    let four_year_period = day_of_century / 1_461;
    let day_of_period = day_of_century % 1_461;
    // A leap day is the last day of year 3, so it must not advance the year.
    let year_of_period = (day_of_period / 365).min(3);
    let day_of_year = day_of_period - year_of_period * 365;
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let jan_or_feb = march_month >= 10;
    let year = era * 400 + century * 100 + four_year_period * 4 + year_of_period;
    let month = if jan_or_feb {
        march_month - 9
    } else {
        march_month + 3
    };
    (year + i64::from(jan_or_feb), month as u8, day as u8)
}

impl fmt::Display for LogTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (date, clock) = self.to_local(UtcOffset(0));
        write!(f, "{date} {clock}")
    }
}

impl fmt::Display for LogDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { year, month, day } = *self;
        if year < 0 {
            write!(f, "-{:04}-{month:02}-{day:02}", year.unsigned_abs())
        } else {
            write!(f, "{year:04}-{month:02}-{day:02}")
        }
    }
}

impl fmt::Display for LocalClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            hour,
            minute,
            second,
            millisecond,
        } = *self;
        write!(f, "{hour:02}:{minute:02}:{second:02}.{millisecond:03}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn date(year: i32, month: u8, day: u8) -> LogDate {
        LogDate { year, month, day }
    }

    #[test]
    fn converts_local_time_to_independent_utc_value() {
        // 56 complete years including 14 leap days, then 330 days in 2026.
        const DAYS: i64 = 56 * 365 + 14 + 330;
        const UTC_MILLIS: i64 = (((DAYS * 24 + 13) * 60 + 53) * 60 + 10) * 1_000 + 123;
        let at = LogTime(UTC_MILLIS);
        let local_date = date(2026, 11, 27);
        let offset = UtcOffset(540);

        assert_eq!(
            LogTime::from_local(local_date, 22, 53, 10, 123, offset),
            Some(at)
        );
        assert_eq!(at.to_string(), "2026-11-27 13:53:10.123");
        let (converted_date, clock) = at.to_local(offset);
        assert_eq!(converted_date, local_date);
        assert_eq!(clock.to_string(), "22:53:10.123");
    }

    #[test]
    fn applies_negative_and_fractional_offsets_across_year_boundary() {
        let west = LogTime::from_local(date(2026, 12, 31), 23, 0, 0, 0, UtcOffset(-300))
            .expect("valid local date and clock");
        assert_eq!(west.to_string(), "2027-01-01 04:00:00.000");
        assert_eq!(west.to_local(UtcOffset(-300)).0, date(2026, 12, 31));

        let east = LogTime::from_local(date(2027, 1, 1), 1, 15, 0, 0, UtcOffset(330))
            .expect("valid local date and clock");
        assert_eq!(east.to_string(), "2026-12-31 19:45:00.000");
        assert_eq!(east.to_local(UtcOffset(330)).0, date(2027, 1, 1));
    }

    #[test]
    fn handles_leap_days_and_centuries() {
        let day = date(2028, 2, 29);
        let at =
            LogTime::from_local(day, 23, 59, 59, 999, UtcOffset(0)).expect("2028 has a leap day");
        assert_eq!(at.to_string(), "2028-02-29 23:59:59.999");
        assert_eq!(LogTime(at.0 + 1).to_string(), "2028-03-01 00:00:00.000");
        assert_eq!(date(2100, 2, 29).days_since_epoch(), None);
        assert_eq!(
            date(2000, 2, 29)
                .days_since_epoch()
                .and_then(LogDate::from_days_since_epoch),
            Some(date(2000, 2, 29))
        );
    }

    #[test]
    fn round_trips_first_day_of_each_month() {
        for year in 1970..=2100 {
            for month in 1..=12 {
                let day = date(year, month, 1);
                assert_eq!(
                    day.days_since_epoch()
                        .and_then(LogDate::from_days_since_epoch),
                    Some(day)
                );
            }
        }
    }

    #[test]
    fn supports_negative_timestamps_and_extreme_offsets() {
        assert_eq!(LogTime(-1).to_string(), "1969-12-31 23:59:59.999");
        for at in [LogTime(i64::MIN), LogTime(i64::MAX)] {
            for offset in [UtcOffset(i16::MIN), UtcOffset(i16::MAX)] {
                let (date, clock) = at.to_local(offset);
                assert_eq!(
                    LogTime::from_local(
                        date,
                        clock.hour,
                        clock.minute,
                        clock.second,
                        clock.millisecond,
                        offset
                    ),
                    Some(at)
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_dates_and_clock_fields() {
        let offset = UtcOffset(0);
        for invalid in [date(2026, 13, 1), date(2026, 4, 31), date(2026, 1, 0)] {
            assert_eq!(LogTime::from_local(invalid, 0, 0, 0, 0, offset), None);
        }
        for (hour, minute, second, millis) in
            [(24, 0, 0, 0), (0, 60, 0, 0), (0, 0, 60, 0), (0, 0, 0, 1000)]
        {
            assert_eq!(
                LogTime::from_local(date(2026, 1, 1), hour, minute, second, millis, offset),
                None
            );
        }
        assert_eq!(LogDate::from_days_since_epoch(i64::MIN), None);
        assert_eq!(LogDate::from_days_since_epoch(i64::MAX), None);
    }
}
