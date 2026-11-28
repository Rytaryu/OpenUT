use std::ops::Range;

use openut_hublog::decode::LineEnding;
use openut_hublog::header::{ParsedLine, parse_line};

use crate::{Kind, Warning};

pub(crate) struct Span {
    pub range: Range<usize>,
    pub kind: Kind,
}

pub(crate) struct Scan {
    pub spans: Vec<Span>,
    pub warnings: Vec<Warning>,
}

pub(crate) fn scan(text: &str, pc_name: Option<&str>) -> Scan {
    let mut scan = Scan {
        spans: Vec::new(),
        warnings: Vec::new(),
    };
    let pc_name = pc_name
        // A supplied name cannot consume a delimiter. The residual check still scans for it.
        .filter(|name| !name.is_empty() && !name.contains(['\r', '\n']))
        .map(str::to_ascii_lowercase);
    let separator = LineEnding::detect(text).unwrap_or(LineEnding::Lf).as_str();
    let mut offset = 0;
    for (index, raw_record) in text.split_inclusive(separator).enumerate() {
        let record = raw_record
            .strip_suffix("\r\n")
            .or_else(|| raw_record.strip_suffix(separator))
            .unwrap_or(raw_record);
        let mut found = RecordSpans::default();
        if let ParsedLine::Entry(entry) = parse_line(record)
            && entry.func == "BiCollector::GetPcSerialNumber"
        {
            let body = entry.body;
            let mut line_start = record.len() - body.len();
            for part in body.split_inclusive(['\r', '\n']) {
                let body = part.strip_suffix(['\r', '\n']).unwrap_or(part);
                found.add(line_start..line_start + body.len(), Kind::PcSerialBody);
                line_start += part.len();
            }
        } else {
            find_record_spans(record, pc_name.as_deref(), &mut found);
        }
        found.spans.sort_by_key(|span| span.range.start);
        scan.spans.extend(found.spans.into_iter().map(|span| Span {
            range: span.range.start + offset..span.range.end + offset,
            kind: span.kind,
        }));
        for kind in found.suspicious {
            scan.warnings.push(Warning {
                record: index + 1,
                kind,
            });
        }
        offset += raw_record.len();
    }
    scan
}

#[derive(Default)]
struct RecordSpans {
    spans: Vec<Span>,
    suspicious: Vec<Kind>,
}

impl RecordSpans {
    fn add(&mut self, range: Range<usize>, kind: Kind) {
        if !range.is_empty()
            && !self
                .spans
                .iter()
                .any(|span| range.start < span.range.end && span.range.start < range.end)
        {
            self.spans.push(Span { range, kind });
        }
    }

    fn warn(&mut self, kind: Kind) {
        if !self.suspicious.contains(&kind) {
            self.suspicious.push(kind);
        }
    }

    fn covers(&self, range: Range<usize>) -> bool {
        self.spans
            .iter()
            .any(|span| span.range.start <= range.start && range.end <= span.range.end)
    }
}

fn find_record_spans(record: &str, pc_name: Option<&str>, found: &mut RecordSpans) {
    let lower = record.to_ascii_lowercase();
    find_wifi_spans(record, &lower, found);

    for tail in unmasked_markers(&lower, "sn:", found) {
        match delimited(record, tail, '(', ')') {
            Some(range) if known_serial(&record[range.clone()]) => {
                found.add(range, Kind::TrackerSerial);
            }
            _ => found.warn(Kind::TrackerSerial),
        }
    }
    for tail in unmasked_markers(&lower, "tracker not found. tracker:", found) {
        let candidate = value_start(record, tail);
        let len = candidate
            .find(|ch: char| ch.is_whitespace() || ch == ',')
            .unwrap_or(candidate.len());
        let start = record.len() - candidate.len();
        if known_serial(&candidate[..len]) {
            found.add(start..start + len, Kind::TrackerSerial);
        } else {
            found.warn(Kind::TrackerSerial);
        }
    }

    for tail in unmasked_markers(&lower, "bt mac address:", found) {
        match bt_address_range(record, tail) {
            Some(range) => found.add(range, Kind::RadioAddress),
            None => found.warn(Kind::RadioAddress),
        }
    }

    for (index, marker) in lower.match_indices("\\users\\") {
        let start = index + marker.len();
        if let Some(len) = record[start..].find(['\\', '\r', '\n'])
            && record.as_bytes()[start + len] == b'\\'
        {
            found.add(start..start + len, Kind::UserFolder);
        } else {
            found.warn(Kind::UserFolder);
        }
    }

    if let Some(name) = pc_name {
        for (index, _) in lower.match_indices(name) {
            found.add(index..index + name.len(), Kind::PcName);
        }
    }

    warn_about_unknown_shapes(record, &lower, found);
}

fn find_wifi_spans(record: &str, lower: &str, found: &mut RecordSpans) {
    let mut fields = Vec::new();
    for (key, kind) in [
        ("ssid", Kind::WifiName),
        ("password", Kind::WifiPassword),
        ("psk", Kind::WifiPassword),
    ] {
        for end in
            after_markers(lower, key).filter(|end| has_end_boundary(lower, *end, is_field_char))
        {
            fields.push((end - key.len(), end, kind));
        }
    }
    fields.sort_by_key(|&(start, _, _)| start);
    for (start, end, kind) in fields {
        if found.covers(start..end) {
            continue;
        }
        let end = match record[..start].chars().next_back() {
            Some(quote @ ('"' | '\'')) if record[end..].starts_with(quote) => end + 1,
            _ => end,
        };
        let rest = value_start(record, end);
        let value_after = if let Some(value) = rest.strip_prefix([':', '=']) {
            record.len() - value.len()
        } else if rest.starts_with(['"', '\'', '(']) {
            end
        } else {
            // The #7 sample logs did not contain Wi-Fi credentials. A bare
            // keyword without a separator is ambiguous, so leave it for review.
            if !rest.trim().is_empty() {
                found.warn(kind);
            }
            continue;
        };
        if let Some(range) = wifi_value(record, value_after) {
            found.add(range, kind);
        } else {
            found.warn(kind);
        }
    }
}

fn unmasked_markers(text: &str, marker: &str, found: &RecordSpans) -> Vec<usize> {
    after_markers(text, marker)
        .filter(|end| !found.covers(end - marker.len()..*end))
        .collect()
}

fn after_markers<'a>(text: &'a str, marker: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(marker)
        .filter(|(index, _)| has_start_boundary(text, *index, is_field_char))
        .map(|(index, _)| index + marker.len())
}

fn is_field_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn has_start_boundary(text: &str, index: usize, continues: impl Fn(char) -> bool) -> bool {
    text[..index]
        .chars()
        .next_back()
        .is_none_or(|ch| !continues(ch))
}

fn has_end_boundary(text: &str, end: usize, continues: impl Fn(char) -> bool) -> bool {
    text[end..].chars().next().is_none_or(|ch| !continues(ch))
}

fn value_start(text: &str, after: usize) -> &str {
    text[after..].trim_start_matches([' ', '\t'])
}

fn delimited(text: &str, after: usize, open: char, close: char) -> Option<Range<usize>> {
    let rest = value_start(text, after).strip_prefix(open)?;
    let len = rest.find([close, '\r', '\n'])?;
    if !rest[len..].starts_with(close) {
        return None;
    }
    let start = text.len() - rest.len();
    Some(start..start + len)
}

fn known_serial(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 13
        && bytes[..2].iter().all(u8::is_ascii_digit)
        && bytes[2..4].iter().all(u8::is_ascii_uppercase)
        && bytes[4..].iter().all(u8::is_ascii_digit)
}

fn bt_address_range(text: &str, after: usize) -> Option<Range<usize>> {
    let slot = delimited(text, after, '(', ')')?;
    let number = text[slot.clone()].strip_prefix('#')?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let address = delimited(text, slot.end + 1, '(', ')')?;
    radio_address_shape(&text[address.clone()], ' ').then_some(address)
}

fn radio_address_shape(value: &str, separator: char) -> bool {
    let mut groups = value.split(separator);
    (0..6).all(|_| {
        groups.next().is_some_and(|part| {
            part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    }) && groups.next().is_none()
}

fn wifi_value(text: &str, after: usize) -> Option<Range<usize>> {
    let value = value_start(text, after);
    let first = value.chars().next()?;
    if first == '(' {
        return delimited(text, after, '(', ')');
    }
    if matches!(first, '\'' | '"') {
        let body = &value[1..];
        let mut escaped = false;
        for (index, ch) in body.char_indices() {
            match ch {
                '\r' | '\n' => return None,
                ch if ch == first && !escaped => {
                    let start = text.len() - body.len();
                    return Some(start..start + index);
                }
                '\\' if !escaped => escaped = true,
                _ => escaped = false,
            }
        }
        return None;
    }
    let field = value.split([',', ';', ')', ']', '}', '\r', '\n']).next()?;
    // Keep spaces in an unquoted value up to the next field or record separator.
    let end = field
        .char_indices()
        .filter(|&(_, ch)| matches!(ch, ' ' | '\t'))
        .find(|&(index, _)| starts_assignment(field[index..].trim_start_matches([' ', '\t'])))
        .map_or(field.len(), |(index, _)| index);
    let len = field[..end].trim_end_matches([' ', '\t']).len();
    let start = text.len() - value.len();
    (len != 0).then_some(start..start + len)
}

fn starts_assignment(text: &str) -> bool {
    if !text.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') {
        return false;
    }
    let end = text
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
        .unwrap_or(text.len());
    text[end..]
        .trim_start_matches([' ', '\t'])
        .starts_with([':', '='])
}

fn warn_about_unknown_shapes(record: &str, lower: &str, found: &mut RecordSpans) {
    if lower.contains("bicollector::getpcserialnumber") {
        found.warn(Kind::PcSerialBody);
    }
    let separator = |ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '-');
    let mut offset = 0;
    for raw in record.split_inclusive(separator) {
        let token = raw.trim_end_matches(separator);
        let value = token.trim_matches([':', '-']);
        let start = offset + token.len() - token.trim_start_matches([':', '-']).len();
        if !value.is_empty() && !found.covers(start..start + value.len()) {
            if known_serial(value) {
                found.warn(Kind::TrackerSerial);
            } else if radio_address_shape(value, ':') || radio_address_shape(value, '-') {
                found.warn(Kind::RadioAddress);
            }
        }
        offset += raw.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected<'a>(record: &'a str, scan: &Scan) -> Vec<(Kind, &'a str)> {
        scan.spans
            .iter()
            .map(|span| (span.kind, &record[span.range.clone()]))
            .collect()
    }

    #[test]
    fn selects_unquoted_credentials_without_eating_other_fields() {
        let record =
            "ssid: Test Network, password: dummy secret, retries: 2; psk=other test key mode=1";
        let scan = scan(record, None);
        assert_eq!(
            selected(record, &scan),
            [
                (Kind::WifiName, "Test Network"),
                (Kind::WifiPassword, "dummy secret"),
                (Kind::WifiPassword, "other test key"),
            ]
        );
        assert!(scan.warnings.is_empty());
    }

    #[test]
    fn ignores_field_markers_within_a_quoted_credential() {
        let record = r#"password="dummy \"ssid: Test Network, sn: (00XX000000001)\"", psk=dummy"#;
        let scan = scan(record, None);
        let spans = selected(record, &scan);
        assert_eq!(spans.len(), 2);
        assert_eq!(
            spans[0].1,
            r#"dummy \"ssid: Test Network, sn: (00XX000000001)\""#
        );
        assert_eq!(spans[1], (Kind::WifiPassword, "dummy"));
        assert!(scan.warnings.is_empty());
    }

    #[test]
    fn leaves_quoted_key_names_and_delimiters_intact() {
        let record = r#"{"ssid":"Test Network","password":"dummy secret"}"#;
        let scan = scan(record, None);
        assert_eq!(
            selected(record, &scan),
            [
                (Kind::WifiName, "Test Network"),
                (Kind::WifiPassword, "dummy secret")
            ]
        );
        assert!(scan.warnings.is_empty());
    }

    #[test]
    fn scans_literal_pc_name_inside_a_larger_token() {
        let record = "host peer_synthetic-pc and SYNTHETIC-PC.example";
        let scan = scan(record, Some("Synthetic-PC"));
        assert_eq!(
            selected(record, &scan),
            [
                (Kind::PcName, "synthetic-pc"),
                (Kind::PcName, "SYNTHETIC-PC")
            ]
        );
    }

    #[test]
    fn removes_pc_serial_body_segments_without_removing_line_endings() {
        let record = "# Log begin\r\n[2026-11-27 22:53:34.900 +9 0xF88 ERROR BiCollector::GetPcSerialNumber] [3976] test\ncontinued\rthird\r\nnext";
        let scan = scan(record, None);
        assert_eq!(
            selected(record, &scan),
            [
                (Kind::PcSerialBody, "test"),
                (Kind::PcSerialBody, "continued"),
                (Kind::PcSerialBody, "third"),
            ]
        );
        assert!(scan.warnings.is_empty());
    }

    #[test]
    fn the_first_lf_determines_record_numbers_in_a_mixed_log() {
        let record =
            "# Log begin\nordinary\nordinary\r\nTRACKER NOT FOUND. tracker: FUTURE-HW001\r\n";
        assert_eq!(
            scan(record, None).warnings,
            [Warning {
                record: 4,
                kind: Kind::TrackerSerial,
            }]
        );
    }

    #[test]
    fn warns_about_malformed_and_unclassified_identifiers_by_record() {
        let record = "ordinary\r\nTRACKER NOT FOUND. tracker: FUTURE-HW001\r\nssid: \"unterminated\r\nunknown 02:00:00:00:00:42";
        let scan = scan(record, None);
        assert!(scan.spans.is_empty());
        assert_eq!(
            scan.warnings
                .iter()
                .map(|warning| (warning.record, warning.kind))
                .collect::<Vec<_>>(),
            [
                (2, Kind::TrackerSerial),
                (3, Kind::WifiName),
                (4, Kind::RadioAddress)
            ]
        );
    }
}
