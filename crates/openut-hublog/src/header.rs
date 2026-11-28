//! Parsing of an already decoded Hub log line (watcher design §4.3).

use openut_core::time::{LogDate, LogTime, UtcOffset};

pub use crate::offset_fill::{FileKind, FilledEntry, OffsetFill};

/// A log record, a metadata line, or the original text of a malformed record.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedLine<'a> {
    Entry(Entry<'a>),
    Meta(&'a str),
    NoHeader(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub at: LogTime,
    pub offset: UtcOffset,
    /// Whether the header supplied a readable UTC offset. The parser uses zero otherwise.
    pub offset_known: bool,
    pub level: &'a str,
    pub func: &'a str,
    pub thread: Option<&'a str>,
    pub body: &'a str,
}

pub fn parse_line(line: &str) -> ParsedLine<'_> {
    if line.starts_with('#') {
        return ParsedLine::Meta(line);
    }
    match parse_entry(line) {
        Some(parts) => ParsedLine::Entry(parts.entry),
        None => ParsedLine::NoHeader(line),
    }
}

/// Text to check for private numeric identifiers. Validated numeric metadata is
/// excluded, while free text and unrecognized fields remain searchable.
pub fn unclassified_numeric_fields(line: &str) -> impl Iterator<Item = &str> {
    let fields = if let Some(parts) = parse_entry(line).filter(|_| {
        line.split_once(']')
            .is_some_and(|(header, _)| !header.contains(['\r', '\n']))
    }) {
        let entry = parts.entry;
        let offset = if entry.offset_known { "" } else { parts.offset };
        let identifier = if parts
            .identifier
            .strip_prefix("0x")
            .or_else(|| parts.identifier.strip_prefix("0X"))
            .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            ""
        } else {
            parts.identifier
        };
        [offset, identifier, entry.level, entry.func, entry.body]
    } else if is_numeric_metadata(line) {
        [""; 5]
    } else {
        [line, "", "", "", ""]
    };
    fields.into_iter().filter(|field| !field.is_empty())
}

fn is_numeric_metadata(line: &str) -> bool {
    let Some(text) = line.strip_prefix("# ") else {
        return false;
    };
    if let Some(timestamp) = text
        .strip_prefix("Log begin at ")
        .or_else(|| text.strip_prefix("Log rotate by date at "))
    {
        return timestamp
            .split_once(' ')
            .is_some_and(|(date, clock)| parse_timestamp(date, clock, UtcOffset(0)).is_some());
    }
    text.split_once('=').is_some_and(|(key, value)| {
        matches!(
            key,
            "WriteTimeInterval"
                | "OpenType"
                | "FileType"
                | "EncodeType"
                | "Truncate"
                | "MaxFileSize"
                | "MaxRollingCount"
                | "WriteThrough"
                | "HardFlush"
                | "FilenameWithProcessID"
                | "InitialLogLevel"
                | "ThreadPriority"
                | "AllPackagesAccess"
        ) && !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
    })
}

struct EntryParts<'a> {
    entry: Entry<'a>,
    offset: &'a str,
    identifier: &'a str,
}

fn parse_entry(line: &str) -> Option<EntryParts<'_>> {
    let (header, tail) = line.strip_prefix('[')?.split_once(']')?;
    if header.starts_with(char::is_whitespace) {
        return None;
    }
    let (date, rest) = take_token(header);
    let (clock, rest) = take_token(rest);
    let (offset_token, after_offset) = take_token(rest);
    let (offset, offset_text, rest) = if offset_token.starts_with(['+', '-']) {
        (parse_offset(offset_token), offset_token, after_offset)
    } else {
        (None, "", rest)
    };
    let offset_known = offset.is_some();
    let offset = offset.unwrap_or(UtcOffset(0));
    let at = parse_timestamp(date, clock, offset)?;
    let (identifier, rest) = take_token(rest);
    let (level, func) = take_token(rest);

    // Consume only the field separator, leaving the body's own leading whitespace.
    let (thread, body) = match tail.strip_prefix([' ', '\t']) {
        Some(after_header) => take_thread(after_header)
            .map_or((None, after_header), |(thread, body)| {
                (Some(thread), body.strip_prefix([' ', '\t']).unwrap_or(body))
            }),
        None => (None, tail),
    };

    Some(EntryParts {
        entry: Entry {
            at,
            offset,
            offset_known,
            level,
            func: func.trim_end(),
            thread,
            body,
        },
        offset: offset_text,
        identifier,
    })
}

fn take_thread(text: &str) -> Option<(&str, &str)> {
    let (thread, rest) = text.strip_prefix('[')?.split_once(']')?;
    (!thread.is_empty() && thread.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some((thread, rest))
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
                offset_known: true,
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
            assert!(parsed.offset_known);
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
    fn preserves_body_prefixes_when_the_thread_field_is_missing() {
        for body in [
            "[ServiceStatusToVHServer] status",
            "  [2] OET OVERSPEED!",
            "\n.",
        ] {
            let line = format!("[2026-11-27 22:53:10.123 +9 0x0 INFO f] {body}");
            let parsed = entry(&line);
            assert_eq!(parsed.thread, None);
            assert_eq!(parsed.body, body);
        }
    }

    #[test]
    fn preserves_sample_header_fields_and_body_whitespace() {
        let examples = [
            (
                "[2026-11-28 00:00:53.613 +9 0x3410 INFO ] [13328] ==================== Log Start ====================",
                "INFO",
                "",
                Some("13328"),
                "==================== Log Start ====================",
            ),
            (
                "[2026-11-27 22:53:34.900 +9 0xF88 ERROR BiCollector::GetPcSerialNumber] [3976] ",
                "ERROR",
                "BiCollector::GetPcSerialNumber",
                Some("3976"),
                "",
            ),
            (
                "[2026-11-27 22:53:34.900 +9 0xF88 INFO bisender::BiSender::Init] [3976] init",
                "INFO",
                "bisender::BiSender::Init",
                Some("3976"),
                "init",
            ),
            (
                "[2026-11-27 22:53:34.900 +9 0xF88 INFO CSocketAccept::~CSocketAccept] [3976] end",
                "INFO",
                "CSocketAccept::~CSocketAccept",
                Some("3976"),
                "end",
            ),
            (
                "[2026-11-27 22:53:34.900 +9 0xF88 ERROR OETServer::OnTrackerStatusIndicatorUpdate] [3976] [2] OET OVERSPEED!",
                "ERROR",
                "OETServer::OnTrackerStatusIndicatorUpdate",
                Some("3976"),
                "[2] OET OVERSPEED!",
            ),
            (
                "[2026-11-27 00:00:06.739 +9 0x813C INFO dumpPUMRFDropRate] [33084]   - D[1] ",
                "INFO",
                "dumpPUMRFDropRate",
                Some("33084"),
                "  - D[1] ",
            ),
            (
                "[2026-11-27 19:07:28.939 +9 0x3410 INFO TrackerDongleHID::DataDispatchThreadProc] [13328] Wait for HID data got timeout.\n",
                "INFO",
                "TrackerDongleHID::DataDispatchThreadProc",
                Some("13328"),
                "Wait for HID data got timeout.\n",
            ),
        ];
        for (line, level, func, thread, body) in examples {
            let parsed = entry(line);
            assert_eq!(
                (parsed.level, parsed.func, parsed.thread, parsed.body),
                (level, func, thread, body)
            );
        }
    }

    #[test]
    fn classifies_startup_settings_and_rotation_as_metadata() {
        for line in [
            "# Log begin at 2026-11-28 00:00:53.613",
            "# WriteTimeInterval=5000",
            "# Log rotate by date at 2026-11-27 00:01:03.40",
        ] {
            assert_eq!(parse_line(line), ParsedLine::Meta(line));
        }
    }

    #[test]
    fn falls_back_to_zero_for_missing_or_unreadable_offsets() {
        for offset in ["+5.5 ", "+5:60 ", ""] {
            let line = format!("[2026-11-27 22:53:10.123 {offset}0xF88 INFO f] [1] x");
            let parsed = entry(&line);
            assert!(!parsed.offset_known);
            assert_eq!(parsed.offset, UtcOffset(0));
            assert_eq!(parsed.at.to_string(), "2026-11-27 22:53:10.123");
            assert_eq!(
                (parsed.level, parsed.func, parsed.thread, parsed.body),
                ("INFO", "f", Some("1"), "x")
            );
        }
        for line in [
            "[2026-11-27 22:53:10.123 +5:60]",
            "[2026-11-27 22:53:10.123 9]",
            "[2026-11-27 22:53:10.123]",
        ] {
            let parsed = entry(line);
            assert!(!parsed.offset_known);
            assert_eq!(parsed.offset, UtcOffset(0));
            assert_eq!(parsed.at.to_string(), "2026-11-27 22:53:10.123");
            assert_eq!((parsed.level, parsed.func), ("", ""));
        }
        let known_zero = entry("[2026-11-27 22:53:10.123 +0]");
        assert!(known_zero.offset_known);
        assert_eq!(known_zero.offset, UtcOffset(0));
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
