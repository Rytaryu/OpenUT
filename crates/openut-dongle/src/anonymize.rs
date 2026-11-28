//! Rebuild shareable recordings from the documented allowlist, then check for residual values.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, BufRead, Cursor};

use openut_anonymize::Kind;

use crate::decode::{Decoder, addr, normalize};
use crate::record::{
    OmissionCount, OmittedReason, RawReport, ReadError, Record, RecordReader, RecordWriter,
};

mod packet;

pub struct AnonymizedRecording {
    bytes: Vec<u8>,
    omissions: Vec<OmissionCount>,
    skipped_rows: u64,
}

impl AnonymizedRecording {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn omissions(&self) -> &[OmissionCount] {
        &self.omissions
    }

    pub fn skipped_rows(&self) -> u64 {
        self.skipped_rows
    }
}

#[derive(Debug)]
pub enum ExportError {
    Read(ReadError),
    Io(io::Error),
    InvalidMapping,
    Residual,
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => fmt::Display::fmt(error, f),
            Self::Io(error) => write!(f, "共有用の記録を作成できません（{:?}）", error.kind()),
            Self::InvalidMapping => f.write_str("匿名化の対応表の形式を読み取れません"),
            Self::Residual => f.write_str("共有用の記録の匿名化を確認できませんでした"),
        }
    }
}

impl std::error::Error for ExportError {}

/// Returns shareable bytes only after independently checking the rebuilt recording.
///
/// The optional TSV is the private mapping produced by the Hub log anonymizer.
pub fn anonymize_recording(
    input: impl BufRead,
    mapping: Option<&str>,
) -> Result<AnonymizedRecording, ExportError> {
    rebuild_and_check(input, mapping, packet::rebuild)
}

fn rebuild_and_check<R, P>(
    input: R,
    mapping: Option<&str>,
    mut rebuild: P,
) -> Result<AnonymizedRecording, ExportError>
where
    R: BufRead,
    P: FnMut(
        &[u8],
        Option<crate::record::TrackerLabel>,
        &crate::decode::DongleEvent,
        &mut dyn FnMut(Option<Kind>, &str) -> String,
    ) -> Result<Vec<u8>, OmittedReason>,
{
    let mut replacements = Replacements::from_tsv(mapping)?;
    let mut reader = RecordReader::new(input).map_err(ExportError::Read)?;
    let mut header = reader.header().clone();
    let was_anonymized = header.anonymized;
    let mut decoder = Decoder::for_recording(was_anonymized);
    header.anonymized = true;
    let mut writer = RecordWriter::new(Vec::new(), &header).map_err(ExportError::Io)?;
    let mut originals = OriginalValues::default();
    let mut omissions = BTreeMap::new();
    let mut footer = None;

    while let Some(record) = reader.read_record().map_err(ExportError::Io)? {
        let record = match record {
            Record::Input(raw) => {
                let addr = addr::report_address(normalize(&raw.bytes));
                originals.collect(&raw.bytes, addr, was_anonymized);
                let event = decoder.decode(&raw.bytes);
                let tracker = packet::tracker(&raw.bytes, &mut decoder);
                match rebuild(&raw.bytes, tracker, &event, &mut |kind, value| {
                    replacements.replace(kind, value)
                }) {
                    Ok(bytes) => Record::Input(RawReport { bytes, ..raw }),
                    Err(reason) => {
                        let report_type = packet::report_type(&raw.bytes);
                        Record::Omitted {
                            mono_us: raw.mono_us,
                            wall_ms: raw.wall_ms,
                            report_type,
                            len: raw.bytes.len(),
                            tracker: (report_type == 0x28).then_some(tracker).flatten(),
                            reason,
                        }
                    }
                }
            }
            Record::Footer { .. } => {
                footer = Some(record);
                continue;
            }
            Record::Omitted {
                mono_us,
                wall_ms,
                report_type,
                len,
                tracker,
                reason,
            } => Record::Omitted {
                mono_us,
                wall_ms,
                report_type,
                len,
                tracker: tracker.filter(|label| label.0.is_ascii_uppercase()),
                reason,
            },
            record => record,
        };
        // A footer before a later typed row is not the recording's end.
        footer = None;
        if let Record::Omitted {
            report_type,
            reason,
            ..
        } = &record
        {
            let count = omissions.entry((*report_type, *reason)).or_insert(0_u64);
            *count = count.saturating_add(1);
        }
        writer.write_record(&record).map_err(ExportError::Io)?;
    }

    let omissions: Vec<_> = omissions
        .into_iter()
        .map(|((report_type, reason), count)| OmissionCount {
            report_type,
            reason,
            count,
        })
        .collect();
    let stats = reader.stats();
    if let Some(mut footer) = footer {
        if let Record::Footer { time_reversed, .. } = &mut footer {
            *time_reversed = (*time_reversed).max(stats.time_reversed);
        }
        writer
            .write_export_footer(&footer, &omissions)
            .map_err(ExportError::Io)?;
    }
    let bytes = writer.into_inner();
    if replacements.failed || originals.find_in(&bytes)? {
        return Err(ExportError::Residual);
    }
    Ok(AnonymizedRecording {
        bytes,
        omissions,
        skipped_rows: stats
            .malformed
            .saturating_sub(stats.time_reversed)
            .saturating_add(stats.unknown),
    })
}

#[derive(Default)]
struct OriginalValues {
    binary: BTreeSet<Vec<u8>>,
    text: BTreeSet<Vec<u8>>,
}

impl OriginalValues {
    fn collect(&mut self, report: &[u8], address: Option<&[u8; 6]>, was_anonymized: bool) {
        if let Some(address) = address
            && (!was_anonymized || addr::anonymized_label(address).is_none())
        {
            self.binary.insert(address.to_vec());
            self.binary.insert(address[1..].to_vec());
        }
        for value in packet::private_values(report) {
            if value.len() >= 4 {
                self.text.insert(value.to_vec());
            }
        }
    }

    fn find_in(&self, bytes: &[u8]) -> Result<bool, ExportError> {
        let mut rebuilt = RecordReader::new(Cursor::new(bytes)).map_err(ExportError::Read)?;
        let patterns = self.binary.iter().chain(&self.text).map(Vec::as_slice);
        let hex_patterns: Vec<_> = patterns.clone().map(hex).collect();
        let raw_patterns = Patterns::new(patterns);
        let encoded_patterns = Patterns::new(hex_patterns.iter().map(Vec::as_slice));
        // The header's tool field is the only free text outside reports.
        // The reader has already decoded its JSON escapes.
        let tool = rebuilt.header().tool.as_bytes();
        if raw_patterns.find(tool) || encoded_patterns.find(&tool.to_ascii_lowercase()) {
            return Ok(true);
        }
        while let Some(record) = rebuilt.read_record().map_err(ExportError::Io)? {
            if let Record::Input(raw) = record
                && (raw_patterns.find(&raw.bytes)
                    || encoded_patterns.find(&raw.bytes.to_ascii_lowercase())
                    || encoded_patterns.find(&hex(&raw.bytes)))
            {
                // Canonical lower-case hex also checks case-insensitively and
                // includes matches that start at the second nibble of a byte.
                return Ok(true);
            }
        }
        let stats = rebuilt.stats();
        if stats.malformed != 0 || stats.unknown != 0 {
            return Err(ExportError::Residual);
        }
        Ok(false)
    }
}

struct Patterns<'a> {
    by_prefix: BTreeMap<[u8; 4], Vec<&'a [u8]>>,
}

impl<'a> Patterns<'a> {
    fn new(patterns: impl Iterator<Item = &'a [u8]>) -> Self {
        let mut by_prefix = BTreeMap::<_, Vec<_>>::new();
        for pattern in patterns {
            if let Some(prefix) = pattern.first_chunk::<4>() {
                by_prefix.entry(*prefix).or_default().push(pattern);
            }
        }
        Self { by_prefix }
    }

    fn find(&self, bytes: &[u8]) -> bool {
        if self.by_prefix.is_empty() {
            return false;
        }
        bytes.windows(4).enumerate().any(|(offset, window)| {
            self.by_prefix.get(window).is_some_and(|patterns| {
                patterns
                    .iter()
                    .any(|pattern| bytes[offset..].starts_with(pattern))
            })
        })
    }
}

fn hex(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0xf)]])
        .collect()
}

#[derive(Default)]
struct Replacements {
    preferred: BTreeMap<(Kind, String), String>,
    assigned: BTreeMap<(Option<Kind>, String), String>,
    used: BTreeSet<String>,
    next: BTreeMap<usize, u64>,
    failed: bool,
}

impl Replacements {
    fn from_tsv(mapping: Option<&str>) -> Result<Self, ExportError> {
        let mut result = Self::default();
        let Some(mapping) = mapping else {
            return Ok(result);
        };
        let mut lines = mapping.lines();
        if lines.next() != Some("kind\toriginal\treplacement") {
            return Err(ExportError::InvalidMapping);
        }
        for line in lines {
            let mut columns = line.split('\t');
            let (Some(kind), Some(original), Some(replacement), None) = (
                columns.next(),
                columns.next(),
                columns.next(),
                columns.next(),
            ) else {
                return Err(ExportError::InvalidMapping);
            };
            let kind = match kind {
                "TRACKER" => Kind::TrackerSerial,
                "WIFI" => Kind::WifiName,
                "WIFI-SECRET" => Kind::WifiPassword,
                _ => continue,
            };
            // Non-ASCII values cannot occur in a reconstructable packet.
            if let Some(original) = unescape_printable(original) {
                let key = (kind, replacement_key(Some(kind), &original));
                if result
                    .preferred
                    .insert(key, replacement.to_owned())
                    .is_some_and(|old| old != replacement)
                {
                    return Err(ExportError::InvalidMapping);
                }
            }
        }
        Ok(result)
    }

    fn replace(&mut self, kind: Option<Kind>, original: &str) -> String {
        if original.is_empty() {
            return String::new();
        }
        let key = (kind, replacement_key(kind, original));
        if let Some(replacement) = self.assigned.get(&key) {
            return replacement.clone();
        }
        let preferred = kind
            .and_then(|kind| self.preferred.get(&(kind, key.1.clone())))
            .and_then(|replacement| fit_replacement(replacement, original.len()))
            .filter(|candidate| self.available(candidate, &key.1));
        let replacement = preferred.or_else(|| self.generate(original.len(), &key.1));
        let Some(replacement) = replacement else {
            self.failed = true;
            return "X".repeat(original.len());
        };
        self.used.insert(replacement.clone());
        self.assigned.insert(key, replacement.clone());
        replacement
    }

    fn generate(&mut self, len: usize, original: &str) -> Option<String> {
        let mut index = self.next.get(&len).copied().unwrap_or(1);
        loop {
            let ordinal = base36(index);
            // Width is fixed. Exhaustion must fail rather than reuse a replacement.
            if ordinal.len() > len {
                return None;
            }
            let candidate = fit_replacement(&ordinal, len)?;
            index = index.checked_add(1)?;
            if self.available(&candidate, original) {
                self.next.insert(len, index);
                return Some(candidate);
            }
        }
    }

    fn available(&self, candidate: &str, original: &str) -> bool {
        !candidate.eq_ignore_ascii_case(original) && !self.used.contains(candidate)
    }
}

fn replacement_key(kind: Option<Kind>, value: &str) -> String {
    if kind == Some(Kind::TrackerSerial) {
        value.to_ascii_lowercase()
    } else {
        value.to_owned()
    }
}

fn fit_replacement(value: &str, len: usize) -> Option<String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_alphanumeric())
    {
        return None;
    }
    // Preserve the distinguishing suffix of Hub aliases when the field is short.
    if value.len() >= len {
        Some(value[value.len() - len..].to_owned())
    } else {
        Some(format!("{}{value}", "X".repeat(len - value.len())))
    }
}

fn unescape_printable(value: &str) -> Option<String> {
    let mut result = String::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        let byte = match byte {
            b'\\' => match bytes.next()? {
                unescaped @ (b'\\' | b'\'' | b'"') => unescaped,
                _ => return None,
            },
            byte if (b' '..=b'~').contains(&byte) => byte,
            _ => return None,
        };
        result.push(char::from(byte));
    }
    Some(result)
}

fn base36(mut number: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut reverse = Vec::new();
    loop {
        reverse.push(char::from(DIGITS[(number % 36) as usize]));
        number /= 36;
        if number == 0 {
            break;
        }
    }
    reverse.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Device, Header, Mode};

    fn input(bytes: Vec<u8>) -> Vec<u8> {
        let header = Header {
            tool: "synthetic exporter test".into(),
            mode: Mode::Record,
            started_wall_ms: 0,
            device: Device {
                vid: 0,
                pid: 0,
                interface: 0,
                usage_page: 0,
                usage: 0,
            },
            server_running_at_start: false,
            anonymized: false,
        };
        let mut writer = RecordWriter::new(Vec::new(), &header).expect("in-memory writer");
        writer
            .write_record(&Record::Input(RawReport {
                mono_us: 0,
                wall_ms: 0,
                bytes,
            }))
            .expect("write the synthetic report");
        writer.into_inner()
    }

    #[test]
    fn rejects_residuals_after_a_rebuilder_reintroduces_them() {
        let mut pair = vec![0x18, 12, 0, 0, 0, 0];
        pair.extend([0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);
        let recording = input(pair);
        let verified = anonymize_recording(Cursor::new(&recording), None);
        assert!(verified.is_ok());

        let copied = rebuild_and_check(Cursor::new(&recording), None, |bytes, _, _, _| {
            Ok(bytes.to_vec())
        });
        assert!(matches!(copied, Err(ExportError::Residual)));

        let shifted_hex = rebuild_and_check(Cursor::new(&recording), None, |_, _, _, _| {
            Ok(vec![0xd0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xb9])
        });
        assert!(matches!(shifted_hex, Err(ExportError::Residual)));

        let printable_hex = rebuild_and_check(Cursor::new(&recording), None, |_, _, _, _| {
            Ok(b"0123456789AB".to_vec())
        });
        assert!(matches!(printable_hex, Err(ExportError::Residual)));
    }

    #[test]
    fn uses_the_hub_alias_and_preserves_the_original_width() {
        let tsv = "kind\toriginal\treplacement\nTRACKER\tSYNTHETIC-SEQUENCE\tTRACKER-B\nWIFI\tTEST\\\"NAME\tWIFI-A\n";
        let mut replacements = Replacements::from_tsv(Some(tsv)).expect("private mapping");
        for serial in ["SYNTHETIC-SEQUENCE", "synthetic-sequence"] {
            let replacement = replacements.replace(Some(Kind::TrackerSerial), serial);
            assert!(replacement.ends_with("TRACKER-B"));
            assert_eq!(replacement.len(), serial.len());
        }
        let original = "TEST\"NAME";
        let replacement = replacements.replace(Some(Kind::WifiName), original);
        assert!(replacement.ends_with("WIFI-A"));
        assert_eq!(replacement.len(), original.len());

        let first = replacements.replace(None, "KEEP");
        assert_eq!(replacements.replace(None, "KEEP"), first);
        assert_ne!(replacements.replace(None, "DROP"), first);
    }
}
