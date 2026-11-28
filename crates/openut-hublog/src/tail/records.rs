//! Decode both file streams and normalize their offsets before they are coordinated.

use std::collections::VecDeque;

use openut_core::time::{LogTime, UtcOffset};

use crate::decode::{DecodeStats, Decoder, PositionedLine};
use crate::header::{Entry, FileKind, FilledEntry, OffsetFill, ParsedLine, parse_line};

/// An exclusive byte offset from the start of this file generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordPosition {
    pub name: String,
    pub end: u64,
    pub generation: u64,
    pub decode: DecodeStats,
}

pub(super) struct PositionedRecord {
    pub record: Record,
    pub position: Option<RecordPosition>,
}

/// A record from one of the two file streams, before event classification.
#[derive(Debug, Eq, PartialEq)]
pub struct Record {
    pub file: FileKind,
    pub data: RecordData,
}

impl Record {
    pub fn at(&self) -> Option<LogTime> {
        match &self.data {
            RecordData::Entry(entry) => Some(entry.at),
            RecordData::Meta(_) | RecordData::NoHeader(_) => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RecordData {
    Entry(OwnedEntry),
    Meta(String),
    NoHeader(String),
}

/// A parsed entry stored while waiting for the other file stream.
#[derive(Debug, Eq, PartialEq)]
pub struct OwnedEntry {
    pub at: LogTime,
    pub offset: UtcOffset,
    pub offset_known: bool,
    pub bad_offset: bool,
    pub level: String,
    pub func: String,
    pub thread: Option<String>,
    pub body: String,
}

impl OwnedEntry {
    pub fn as_entry(&self) -> Entry<'_> {
        Entry {
            at: self.at,
            offset: self.offset,
            offset_known: self.offset_known,
            level: &self.level,
            func: &self.func,
            thread: self.thread.as_deref(),
            body: &self.body,
        }
    }
}

impl From<FilledEntry<'_>> for Record {
    fn from(filled: FilledEntry<'_>) -> Self {
        let entry = filled.entry;
        Self {
            file: filled.file,
            data: RecordData::Entry(OwnedEntry {
                at: entry.at,
                offset: entry.offset,
                offset_known: entry.offset_known,
                bad_offset: filled.bad_offset,
                level: entry.level.to_owned(),
                func: entry.func.to_owned(),
                thread: entry.thread.map(str::to_owned),
                body: entry.body.to_owned(),
            }),
        }
    }
}

/// Keeps independent file decoding state and a single offset basis for a session.
pub struct RecordDecoder {
    server: Decoder,
    driver: Decoder,
    offsets: OffsetFill,
    early_driver: EarlyDriver,
}

impl Default for RecordDecoder {
    fn default() -> Self {
        Self {
            server: Decoder::positioned(),
            driver: Decoder::positioned(),
            offsets: OffsetFill::default(),
            early_driver: EarlyDriver::default(),
        }
    }
}

impl RecordDecoder {
    #[cfg(test)]
    pub fn push(&mut self, file: FileKind, bytes: &[u8]) -> Vec<Record> {
        self.push_at(file, "test", 0, bytes)
            .into_iter()
            .map(|row| row.record)
            .collect()
    }

    #[cfg(test)]
    pub fn finish_file(&mut self, file: FileKind) -> Vec<Record> {
        self.finish_at(file, "test", 0)
            .into_iter()
            .map(|row| row.record)
            .collect()
    }

    pub(super) fn push_at(
        &mut self,
        file: FileKind,
        name: &str,
        generation: u64,
        bytes: &[u8],
    ) -> Vec<PositionedRecord> {
        let lines = self.decoder(file).push_positioned(bytes);
        self.parse(file, name, generation, lines)
    }

    /// End a generation at rotation and start the next with no partial bytes.
    /// The shared offset basis survives rotation. Use a new instance on rebuild.
    pub(super) fn finish_at(
        &mut self,
        file: FileKind,
        name: &str,
        generation: u64,
    ) -> Vec<PositionedRecord> {
        let lines =
            std::mem::replace(self.decoder(file), Decoder::positioned()).finish_positioned();
        self.parse(file, name, generation, lines)
    }

    pub fn bad_offset(&self, file: FileKind) -> usize {
        self.offsets.bad_offset(file)
    }

    /// Forget an unfinished driver record after its last file has disappeared.
    pub fn clear_driver_file(&mut self) {
        self.driver = Decoder::positioned();
    }

    fn decoder(&mut self, file: FileKind) -> &mut Decoder {
        match file {
            FileKind::Server => &mut self.server,
            FileKind::Driver => &mut self.driver,
        }
    }

    fn parse(
        &mut self,
        file: FileKind,
        name: &str,
        generation: u64,
        lines: Vec<PositionedLine>,
    ) -> Vec<PositionedRecord> {
        let mut records = Vec::new();
        for PositionedLine { text, end, stats } in lines {
            let position = RecordPosition {
                name: name.to_owned(),
                end,
                generation,
                decode: stats,
            };
            let data = match parse_line(&text) {
                ParsedLine::Entry(entry) => {
                    self.push_entry(file, entry, position, &mut records);
                    continue;
                }
                ParsedLine::Meta(_) => RecordData::Meta(text),
                ParsedLine::NoHeader(_) => RecordData::NoHeader(text),
            };
            let row = PositionedRecord {
                record: Record { file, data },
                position: Some(position),
            };
            if file == FileKind::Driver
                && let Some(group) = self.early_driver.following.back_mut()
            {
                group.following.push(row);
            } else {
                records.push(row);
            }
        }
        records
    }

    fn push_entry(
        &mut self,
        file: FileKind,
        entry: Entry<'_>,
        position: RecordPosition,
        records: &mut Vec<PositionedRecord>,
    ) {
        let before = self.offsets.bad_offset(FileKind::Driver);
        let early = &mut self.early_driver;
        let mut released_driver = Vec::new();
        let mut position = Some(position);
        self.offsets.push(file, entry, |filled| {
            if filled.file == file {
                records.push(PositionedRecord {
                    record: Record::from(filled),
                    position: position.take(),
                });
            } else {
                let delayed = early.following.pop_front().unwrap_or_default();
                released_driver.push(PositionedRecord {
                    record: Record::from(filled),
                    position: delayed.position,
                });
                released_driver.extend(delayed.following);
            }
        });
        if let Some(position) = position {
            // OffsetFill counts evictions immediately, before a basis exists.
            if self.offsets.bad_offset(FileKind::Driver) > before
                && let Some(delayed) = early.following.pop_front()
            {
                records.extend(delayed.following);
            }
            early.following.push_back(EarlyDriverEntry {
                position: Some(position),
                following: Vec::new(),
            });
        }
        records.append(&mut released_driver);
    }
}

/// Keep the untimed records following each driver entry still inside OffsetFill.
/// An untimed prefix can be passed through immediately when no entry is pending.
#[derive(Default)]
struct EarlyDriver {
    following: VecDeque<EarlyDriverEntry>,
}

#[derive(Default)]
struct EarlyDriverEntry {
    position: Option<RecordPosition>,
    following: Vec<PositionedRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_entry(offset: &str, body: &str) -> String {
        format!("[2026-11-28 12:00:00.000 {offset} 0x1 INFO Synthetic] [1] {body}")
    }

    fn labels(records: &[Record]) -> Vec<(FileKind, &str)> {
        records
            .iter()
            .map(|record| {
                let text = match &record.data {
                    RecordData::Entry(entry) => entry.body.as_str(),
                    RecordData::Meta(text) | RecordData::NoHeader(text) => text,
                };
                (record.file, text)
            })
            .collect()
    }

    #[test]
    fn releases_early_driver_entries_and_untimed_rows_after_first_server_entry() {
        let mut decoder = RecordDecoder::default();
        let driver = format!(
            "# driver start\r\n{}\r\n# setting\r\nbroken\r\n{}\r\n",
            log_entry("", "waiting\n"),
            log_entry("+10", "second"),
        );
        let leading = decoder.push(FileKind::Driver, driver.as_bytes());
        assert_eq!(labels(&leading), [(FileKind::Driver, "# driver start")]);

        let server = format!("# server start\n{}\n", log_entry("+9", "first"));
        let records = decoder.push(FileKind::Server, server.as_bytes());
        assert_eq!(
            labels(&records),
            [
                (FileKind::Server, "# server start"),
                (FileKind::Server, "first"),
                (FileKind::Driver, "waiting\n"),
                (FileKind::Driver, "# setting"),
                (FileKind::Driver, "broken"),
                (FileKind::Driver, "second"),
            ]
        );
        assert_eq!(records[1].at(), records[2].at());
        assert!(matches!(&records[2].data, RecordData::Entry(entry) if entry.bad_offset));
        assert!(matches!(&records[4].data, RecordData::NoHeader(_)));
        assert_eq!(decoder.bad_offset(FileKind::Driver), 1);
    }

    #[test]
    fn rotation_finishes_old_generation_without_resetting_other_file_or_offset() {
        let mut decoder = RecordDecoder::default();
        let old = log_entry("+9", "unterminated");
        let utf16: Vec<_> = [0xfeff]
            .into_iter()
            .chain(old.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        for chunk in utf16.chunks(3) {
            assert!(decoder.push(FileKind::Server, chunk).is_empty());
        }
        let driver = log_entry("", "split driver");
        let split = driver.len() - 3;
        assert!(
            decoder
                .push(FileKind::Driver, &driver.as_bytes()[..split])
                .is_empty()
        );

        let ended = decoder.finish_file(FileKind::Server);
        assert_eq!(labels(&ended), [(FileKind::Server, "unterminated")]);
        let server = format!("# new\n{}\n", log_entry("", "new server"));
        let newer = decoder.push(FileKind::Server, server.as_bytes());
        assert_eq!(
            labels(&newer),
            [
                (FileKind::Server, "# new"),
                (FileKind::Server, "new server")
            ]
        );
        assert_eq!(ended[0].at(), newer[1].at());

        let mut rest = driver.as_bytes()[split..].to_vec();
        rest.push(b'\n');
        let driver = decoder.push(FileKind::Driver, &rest);
        assert_eq!(labels(&driver), [(FileKind::Driver, "split driver")]);
        assert_eq!(ended[0].at(), driver[0].at());
    }

    #[test]
    fn overflow_keeps_timed_and_untimed_driver_rows_aligned() {
        let mut decoder = RecordDecoder::default();
        let old = format!("{}\n# after evicted entry\n", log_entry("+10", "evicted"));
        assert!(decoder.push(FileKind::Driver, old.as_bytes()).is_empty());
        let ongoing = format!("{}\n", log_entry("", "kept"));
        for _ in 0..19_999 {
            assert!(
                decoder
                    .push(FileKind::Driver, ongoing.as_bytes())
                    .is_empty()
            );
        }
        let rotated = decoder.push(FileKind::Driver, ongoing.as_bytes());
        assert_eq!(
            labels(&rotated),
            [(FileKind::Driver, "# after evicted entry")]
        );
        assert_eq!(decoder.bad_offset(FileKind::Driver), 1);

        let following = decoder.push(FileKind::Driver, b"# after last entry\n");
        assert!(following.is_empty());
        let server = format!("{}\n", log_entry("+9", "first"));
        let records = decoder.push(FileKind::Server, server.as_bytes());
        assert_eq!(records.len(), 20_002);
        assert_eq!(
            labels(&records[..2]),
            [(FileKind::Server, "first"), (FileKind::Driver, "kept")]
        );
        assert!(
            matches!(&records[1].data, RecordData::Entry(entry) if entry.offset == UtcOffset(600))
        );
        assert_eq!(
            labels(&records[20_001..]),
            [(FileKind::Driver, "# after last entry")]
        );
    }
}
