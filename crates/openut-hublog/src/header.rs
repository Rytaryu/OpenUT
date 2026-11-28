//! Parsing of an already decoded Hub log line (watcher design §4.3).

use openut_core::time::{LogDate, LogTime, UtcOffset};

/// A line with a readable header, or its original text if the header is absent or malformed.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedLine<'a> {
    Entry(Entry<'a>),
    NoHeader(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub at: LogTime,
    pub offset: UtcOffset,
    pub level: &'a str,
    pub func: &'a str,
    pub thread: Option<&'a str>,
    pub body: &'a str,
}

pub fn parse_line(line: &str) -> ParsedLine<'_> {
    match parse_entry(line) {
        Some(entry) => ParsedLine::Entry(entry),
        None => ParsedLine::NoHeader(line),
    }
}

fn parse_entry(line: &str) -> Option<Entry<'_>> {
    let (header, tail) = line.strip_prefix('[')?.split_once(']')?;
    if header.starts_with(char::is_whitespace) {
        return None;
    }
    let (date, rest) = take_token(header);
    let (clock, rest) = take_token(rest);
    let (offset, rest) = take_token(rest);
    let offset = parse_offset(offset)?;
    let at = parse_timestamp(date, clock, offset)?;
    let (_, rest) = take_token(rest); // Skip the opaque identifier.
    let (level, func) = take_token(rest);

    let thread_and_body = tail
        .strip_prefix(char::is_whitespace)
        .and_then(|text| text.trim_start().strip_prefix('['))
        .and_then(|text| text.split_once(']'));
    let (thread, body) =
        thread_and_body.map_or((None, tail), |(thread, body)| (Some(thread), body));

    Some(Entry {
        at,
        offset,
        level,
        func: func.trim_end(),
        thread,
        body: body.trim_start(),
    })
}

fn take_token(text: &str) -> (&str, &str) {
    match text.split_once(char::is_whitespace) {
        Some((token, rest)) => (token, rest.trim_start()),
        None => (text, ""),
    }
}

fn digits(text: &str, width: std::ops::RangeInclusive<usize>) -> Option<u32> {
    if width.contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit()) {
        text.parse().ok()
    } else {
        None
    }
}

fn parse_offset(text: &str) -> Option<UtcOffset> {
    let (sign, rest) = if let Some(rest) = text.strip_prefix('+') {
        (1, rest)
    } else {
        (-1, text.strip_prefix('-')?)
    };
    let (hour, minute) = match rest.split_once(':') {
        Some((hour, minute)) => (hour, digits(minute, 2..=2)?),
        None => (rest, 0),
    };
    let hour = digits(hour, 1..=2)?;
    if minute >= 60 {
        return None;
    }
    Some(UtcOffset(sign * i16::try_from(hour * 60 + minute).ok()?))
}

fn parse_timestamp(date: &str, clock: &str, offset: UtcOffset) -> Option<LogTime> {
    let (year, rest) = date.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    let date = LogDate {
        year: digits(year, 4..=4)?.try_into().ok()?,
        month: digits(month, 2..=2)?.try_into().ok()?,
        day: digits(day, 2..=2)?.try_into().ok()?,
    };
    let (hour, rest) = clock.split_once(':')?;
    let (minute, rest) = rest.split_once(':')?;
    let (second, fraction) = rest.split_once('.')?;
    let micros = digits(fraction, 1..=6)? * 10_u32.pow(6 - fraction.len() as u32);
    let millis = (micros + 500) / 1_000;
    let second_start = LogTime::from_local(
        date,
        digits(hour, 2..=2)?.try_into().ok()?,
        digits(minute, 2..=2)?.try_into().ok()?,
        digits(second, 2..=2)?.try_into().ok()?,
        0,
        offset,
    )?;
    // Rounding can carry into the next second or day.
    Some(LogTime(second_start.0 + i64::from(millis)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(line: &str) -> Entry<'_> {
        let ParsedLine::Entry(entry) = parse_line(line) else {
            panic!("expected a header for {line:?}");
        };
        entry
    }

    #[test]
    fn reads_header_thread_and_body() {
        let line = "[2026-11-27 22:53:10.123 +9 0x1a2b INFO SomeFunc] [12] Add new tracker: (1)";
        assert_eq!(
            entry(line),
            Entry {
                at: LogTime(1_795_787_590_123),
                offset: UtcOffset(540),
                level: "INFO",
                func: "SomeFunc",
                thread: Some("12"),
                body: "Add new tracker: (1)",
            },
        );
    }

    #[test]
    fn rounds_one_through_six_fraction_digits_to_milliseconds() {
        for (fraction, expected) in [
            ("1", "2026-11-27 23:59:59.100"),
            ("12", "2026-11-27 23:59:59.120"),
            ("123", "2026-11-27 23:59:59.123"),
            ("1234", "2026-11-27 23:59:59.123"),
            ("12350", "2026-11-27 23:59:59.124"),
            ("999500", "2026-11-28 00:00:00.000"),
        ] {
            let line = format!("[2026-11-27 23:59:59.{fraction} +0]");
            assert_eq!(entry(&line).at.to_string(), expected);
        }
    }

    #[test]
    fn handles_signed_offsets_with_optional_minutes() {
        for (offset, minutes, utc) in [
            ("+9", 540, "2026-11-27 13:53:10.123"),
            ("+09", 540, "2026-11-27 13:53:10.123"),
            ("-5", -300, "2026-11-28 03:53:10.123"),
            ("+5:30", 330, "2026-11-27 17:23:10.123"),
            ("-0:30", -30, "2026-11-27 23:23:10.123"),
        ] {
            let line = format!("[2026-11-27 22:53:10.123 {offset}]");
            let parsed = entry(&line);
            assert_eq!(parsed.offset, UtcOffset(minutes));
            assert_eq!(parsed.at.to_string(), utc);
        }
    }

    #[test]
    fn preserves_function_spaces_and_allows_missing_optional_fields() {
        let line = "[2026-11-27 22:53:10.123 +9 0x0 INFO Some  Function Name] body";
        let parsed = entry(line);
        assert_eq!(parsed.func, "Some  Function Name");
        assert_eq!(parsed.thread, None);
        assert_eq!(parsed.body, "body");

        let parsed = entry("[2026-11-27 22:53:10.123 +9] [unfinished body");
        assert_eq!(parsed.level, "");
        assert_eq!(parsed.func, "");
        assert_eq!(parsed.thread, None);
        assert_eq!(parsed.body, "[unfinished body");
    }

    #[test]
    fn returns_original_text_for_malformed_headers() {
        for line in [
            "",
            "[",
            "continuation",
            "[2026-13-40 22:53:10.123 +9]",
            "[2026-02-29 22:53:10.123 +9]",
            "[2026-11-27 24:53:10.123 +9]",
            "[2026-11-27 22:53:10.1234567 +9]",
            "[2026-11-27 22:53:10.123 +5:60]",
            "[2026-11-27 22:53:10.123 9]",
            "[2026-11-27 22:53:10.123 +9",
            "[２０２６-11-27 22:53:10.123 +9]",
        ] {
            assert_eq!(parse_line(line), ParsedLine::NoHeader(line));
        }
    }

    #[test]
    fn parses_pseudorandom_text_without_panicking() {
        let alphabet = ['[', ']', ' ', '\t', '-', '+', '0', '9', '.', ':', '時'];
        let mut seed = 0x1954_95c7_u32;
        for length in 0..10_000 {
            let mut line = String::from("[");
            for _ in 0..length % 79 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                line.push(alphabet[(seed >> 16) as usize % alphabet.len()]);
            }
            let _ = parse_line(&line);
        }
    }
}
