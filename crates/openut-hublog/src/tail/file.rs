//! File selection and append-only tracking within one Hub log stream.

use std::collections::BTreeSet;

use openut_core::time::{LogDate, LogTime};

use crate::header::FileKind;

pub const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const PREFIX_BYTES: usize = 4 * 1024;
const LATE_WRITE_WINDOW_MS: i64 = 30_000;

/// The name and observed size of a file in the log directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogFile {
    pub name: String,
    pub len: u64,
}

/// The only I/O needed by the log follower.
pub trait LogSource {
    type Error;

    fn list(&self) -> Result<Vec<LogFile>, Self::Error>;

    /// Read at most `max` bytes, or return `None` if the file is no longer present.
    fn read_at(&self, name: &str, offset: u64, max: usize) -> Result<Option<Vec<u8>>, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogName {
    pub kind: FileKind,
    pub date: LogDate,
    /// A numeric suffix after the date. Report these files without following them.
    pub numbered: bool,
}

/// Match the case-insensitive Hub server and driver filenames, including numbered variants.
pub fn parse_log_name(name: &str) -> Option<LogName> {
    let (stem, extension) = name.rsplit_once('.')?;
    if !extension.eq_ignore_ascii_case("txt") {
        return None;
    }
    let (prefix, suffix) = stem.split_once('_')?;
    let kind = if prefix.eq_ignore_ascii_case("ViveTrackerServer") {
        FileKind::Server
    } else if prefix.eq_ignore_ascii_case("ViveTrackerDriver") {
        FileKind::Driver
    } else {
        return None;
    };
    let (digits, numbered) = if let Some((digits, number)) = suffix.split_once('_') {
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        (digits, true)
    } else {
        (suffix, false)
    };
    if digits.len() != 8 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let date = LogDate {
        year: digits[..4].parse().ok()?,
        month: digits[4..6].parse().ok()?,
        day: digits[6..].parse().ok()?,
    };
    date.days_since_epoch()?;
    Some(LogName {
        kind,
        date,
        numbered,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebuildReason {
    Truncated,
    Replaced,
    Disappeared,
    Restored,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FileEvent {
    Bytes {
        name: String,
        generation: u64,
        bytes: Vec<u8>,
    },
    /// Finish this generation's decoder before handling the next file.
    Finished {
        name: String,
        generation: u64,
    },
    Rotated {
        name: String,
        generation: u64,
        skipped_days: bool,
    },
    LateWrite {
        name: String,
        bytes: u64,
    },
    Missing,
    Numbered {
        name: String,
    },
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    file: &'a LogFile,
    day: i64,
}

#[derive(Clone)]
struct Cursor {
    name: String,
    day: i64,
    generation: u64,
    offset: u64,
    observed_len: u64,
    prefix: Vec<u8>,
}

#[derive(Clone)]
struct RetiredFile {
    name: String,
    len: u64,
    until: LogTime,
}

/// Follow appends to one file stream, with independent generations and rotation.
#[derive(Clone)]
pub struct Tailer {
    kind: FileKind,
    generation: u64,
    cursor: Option<Cursor>,
    caught_up_target: Option<LogFile>,
    retired: Vec<RetiredFile>,
    seen_numbered: BTreeSet<String>,
    found: Option<bool>,
}

impl Tailer {
    pub fn new(kind: FileKind) -> Self {
        Self {
            kind,
            generation: 0,
            cursor: None,
            caught_up_target: None,
            retired: Vec::new(),
            seen_numbered: BTreeSet::new(),
            found: None,
        }
    }

    /// Rewind for a paired rebuild, keeping generation IDs and one-time notices.
    pub fn reset(&mut self) {
        self.cursor = None;
        self.caught_up_target = None;
        self.retired.clear();
        self.found = None;
    }

    pub fn found(&self) -> bool {
        self.found == Some(true)
    }

    /// True once the startup size has been read, or no file is available.
    pub fn caught_up(&self) -> bool {
        self.found.is_some() && self.caught_up_target.is_none()
    }

    /// Whether the listing contains unread bytes or a newer file to move to.
    pub fn has_work(&self, files: &[LogFile]) -> bool {
        let Some(newest) = newest_file(self.kind, files) else {
            return false;
        };
        self.cursor.as_ref().is_none_or(|cursor| {
            find_file(files, &cursor.name).is_some_and(|file| file.len > cursor.offset)
                || newest.day > cursor.day
        })
    }

    /// Check for a rebuild before consuming bytes in either stream.
    pub fn rebuild_reason<S: LogSource + ?Sized>(
        &self,
        source: &S,
        files: &[LogFile],
    ) -> Result<Option<RebuildReason>, S::Error> {
        Ok(self
            .rebuild_details(source, files)?
            .map(|(reason, _)| reason))
    }

    pub(super) fn rebuild_details<S: LogSource + ?Sized>(
        &self,
        source: &S,
        files: &[LogFile],
    ) -> Result<Option<(RebuildReason, String)>, S::Error> {
        let newest = newest_file(self.kind, files);
        if self.kind == FileKind::Driver && newest.is_none() {
            // A completely missing driver stream is allowed without a rebuild.
            return Ok(None);
        }
        if self.kind == FileKind::Server
            && self.found == Some(false)
            && let Some(newest) = newest
        {
            return Ok(Some((RebuildReason::Restored, newest.file.name.clone())));
        }
        let Some(cursor) = &self.cursor else {
            return Ok(None);
        };
        let Some(file) = find_file(files, &cursor.name) else {
            return Ok(Some((RebuildReason::Disappeared, cursor.name.clone())));
        };
        if let Some(target) = self
            .caught_up_target
            .as_ref()
            .filter(|target| find_file(files, &target.name).is_none())
        {
            return Ok(Some((RebuildReason::Disappeared, target.name.clone())));
        }
        if file.len < cursor.offset {
            return Ok(Some((RebuildReason::Truncated, cursor.name.clone())));
        }
        if file.len != cursor.observed_len
            && !cursor.prefix.is_empty()
            && let Some(prefix) = source.read_at(&cursor.name, 0, cursor.prefix.len())?
            && prefix != cursor.prefix
        {
            return Ok(Some((RebuildReason::Replaced, cursor.name.clone())));
        }
        Ok(None)
    }

    /// Advance by at most one bounded content read after a successful rebuild check.
    pub fn read<S: LogSource + ?Sized>(
        &mut self,
        source: &S,
        files: &[LogFile],
        now: LogTime,
    ) -> Result<Vec<FileEvent>, S::Error> {
        let Some(newest) = newest_file(self.kind, files) else {
            let mut events = self.observe_numbered(files);
            if self.found != Some(false) {
                events.push(FileEvent::Missing);
            }
            self.found = Some(false);
            self.cursor = None;
            self.caught_up_target = None;
            self.retired.clear();
            return Ok(events);
        };

        let starting = self.cursor.is_none();
        let (file, day, offset) = if let Some(cursor) = &self.cursor {
            let Some(file) = find_file(files, &cursor.name) else {
                // The caller will retry with a new listing after a read race.
                return Ok(Vec::new());
            };
            (file, cursor.day, cursor.offset)
        } else {
            let first = file_on_day(self.kind, files, newest.day - 1).unwrap_or(newest);
            (first.file, first.day, 0)
        };
        let remaining = file.len.saturating_sub(offset);
        let check_rotation = newest.day > day && remaining <= MAX_READ_BYTES as u64;
        let max = if check_rotation {
            // The old size may have been sampled before the new file appeared.
            MAX_READ_BYTES
        } else {
            remaining.min(MAX_READ_BYTES as u64) as usize
        };
        let mut bytes = if max == 0 {
            Vec::new()
        } else {
            let Some(bytes) = source.read_at(&file.name, offset, max)? else {
                return Ok(Vec::new());
            };
            bytes
        };
        bytes.truncate(max);

        let startup_target = newest.file;
        let refreshed;
        let (files, newest, file) = if check_rotation {
            // Confirm EOF against a size sampled after the content read.
            refreshed = source.list()?;
            let (Some(file), Some(next)) = (
                find_file(&refreshed, &file.name),
                find_file(&refreshed, &newest.file.name),
            ) else {
                return Ok(Vec::new());
            };
            if offset.saturating_add(bytes.len() as u64) > file.len
                || self.rebuild_reason(source, &refreshed)?.is_some()
            {
                // Let the next poll check both streams again before consuming bytes.
                return Ok(Vec::new());
            }
            (
                refreshed.as_slice(),
                Candidate {
                    file: next,
                    day: newest.day,
                },
                file,
            )
        } else {
            (files, newest, file)
        };

        let mut events = self.observe_numbered(files);
        self.collect_late_writes(files, now, &mut events);
        self.found = Some(true);
        if starting {
            self.caught_up_target = Some(startup_target.clone());
            self.start_file(file, day);
        }
        if let Some(cursor) = &mut self.cursor {
            cursor.observed_len = file.len;
            if !bytes.is_empty() {
                let remaining_prefix = PREFIX_BYTES - cursor.prefix.len();
                cursor
                    .prefix
                    .extend_from_slice(&bytes[..bytes.len().min(remaining_prefix)]);
                cursor.offset += bytes.len() as u64;
                events.push(FileEvent::Bytes {
                    name: cursor.name.clone(),
                    generation: cursor.generation,
                    bytes,
                });
            }

            if self.caught_up_target.as_ref().is_some_and(|target| {
                target.name == file.name && cursor.offset >= target.len.min(file.len)
            }) {
                self.caught_up_target = None;
            }

            if cursor.offset >= file.len && newest.day > cursor.day {
                // When a newer file appears during startup, still read the original
                // startup target before following it.
                let next = self
                    .caught_up_target
                    .as_ref()
                    .and_then(|target| candidate(self.kind, find_file(files, &target.name)?))
                    .filter(|target| target.day > cursor.day)
                    .unwrap_or(newest);
                let skipped_days = next.day != cursor.day + 1;
                events.push(FileEvent::Finished {
                    name: cursor.name.clone(),
                    generation: cursor.generation,
                });
                self.retired.push(RetiredFile {
                    name: cursor.name.clone(),
                    len: file.len,
                    until: LogTime(now.0.saturating_add(LATE_WRITE_WINDOW_MS)),
                });
                self.start_file(next.file, next.day);
                events.push(FileEvent::Rotated {
                    name: next.file.name.clone(),
                    generation: self.generation,
                    skipped_days,
                });
            }
        }
        Ok(events)
    }

    fn start_file(&mut self, file: &LogFile, day: i64) {
        self.generation = self.generation.saturating_add(1);
        self.cursor = Some(Cursor {
            name: file.name.clone(),
            day,
            generation: self.generation,
            offset: 0,
            observed_len: file.len,
            prefix: Vec::new(),
        });
        if self
            .caught_up_target
            .as_ref()
            .is_some_and(|target| target.name == file.name && target.len.min(file.len) == 0)
        {
            self.caught_up_target = None;
        }
    }

    fn observe_numbered(&mut self, files: &[LogFile]) -> Vec<FileEvent> {
        files
            .iter()
            .filter(|file| {
                parse_log_name(&file.name)
                    .is_some_and(|name| name.kind == self.kind && name.numbered)
                    && self.seen_numbered.insert(file.name.clone())
            })
            .map(|file| FileEvent::Numbered {
                name: file.name.clone(),
            })
            .collect()
    }

    fn collect_late_writes(
        &mut self,
        files: &[LogFile],
        now: LogTime,
        events: &mut Vec<FileEvent>,
    ) {
        self.retired.retain(|file| file.until >= now);
        for retired in &mut self.retired {
            if let Some(file) = find_file(files, &retired.name)
                && file.len > retired.len
            {
                events.push(FileEvent::LateWrite {
                    name: retired.name.clone(),
                    bytes: file.len - retired.len,
                });
                retired.len = file.len;
            }
        }
    }
}

fn candidate(kind: FileKind, file: &LogFile) -> Option<Candidate<'_>> {
    let name = parse_log_name(&file.name)?;
    (name.kind == kind && !name.numbered).then_some(Candidate {
        file,
        day: name.date.days_since_epoch()?,
    })
}

fn newest_file(kind: FileKind, files: &[LogFile]) -> Option<Candidate<'_>> {
    files
        .iter()
        .filter_map(|file| candidate(kind, file))
        .max_by_key(|file| (file.day, &file.file.name))
}

fn file_on_day(kind: FileKind, files: &[LogFile], day: i64) -> Option<Candidate<'_>> {
    files
        .iter()
        .filter_map(|file| candidate(kind, file))
        .filter(|file| file.day == day)
        .max_by_key(|file| &file.file.name)
}

fn find_file<'a>(files: &'a [LogFile], name: &str) -> Option<&'a LogFile> {
    files.iter().find(|file| file.name == name)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::convert::Infallible;

    use crate::decode::Decoder;

    use super::*;

    const DAY_26: &str = "ViveTrackerServer_20261126.txt";
    const DAY_27: &str = "ViveTrackerServer_20261127.txt";
    const DAY_28: &str = "ViveTrackerServer_20261128.txt";
    const DAY_29: &str = "ViveTrackerServer_20261129.txt";
    const DRIVER_27: &str = "ViveTrackerDriver_20261127.txt";
    const NUMBERED: &str = "ViveTrackerServer_20261127_1.txt";

    #[derive(Default)]
    struct MemorySource {
        files: BTreeMap<String, Vec<u8>>,
        reads: RefCell<Vec<(String, u64, usize)>>,
    }

    impl MemorySource {
        fn write(&mut self, name: &str, bytes: &[u8]) {
            self.files.insert(name.to_owned(), bytes.to_vec());
        }

        fn append(&mut self, name: &str, bytes: &[u8]) {
            self.files.entry(name.to_owned()).or_default().extend(bytes);
        }
    }

    impl LogSource for MemorySource {
        type Error = Infallible;

        fn list(&self) -> Result<Vec<LogFile>, Self::Error> {
            Ok(self
                .files
                .iter()
                .map(|(name, bytes)| LogFile {
                    name: name.clone(),
                    len: bytes.len() as u64,
                })
                .collect())
        }

        fn read_at(
            &self,
            name: &str,
            offset: u64,
            max: usize,
        ) -> Result<Option<Vec<u8>>, Self::Error> {
            self.reads.borrow_mut().push((name.to_owned(), offset, max));
            Ok(self.files.get(name).map(|bytes| {
                let start = (offset as usize).min(bytes.len());
                bytes[start..][..max.min(bytes.len() - start)].to_vec()
            }))
        }
    }

    fn read(tailer: &mut Tailer, source: &MemorySource, now: i64) -> Vec<FileEvent> {
        let files = source.list().expect("memory listing");
        assert_eq!(
            tailer.rebuild_reason(source, &files).expect("memory probe"),
            None
        );
        tailer
            .read(source, &files, LogTime(now))
            .expect("memory read")
    }

    fn bytes(name: &str, generation: u64, content: &[u8]) -> FileEvent {
        FileEvent::Bytes {
            name: name.to_owned(),
            generation,
            bytes: content.to_vec(),
        }
    }

    fn finished(name: &str, generation: u64) -> FileEvent {
        FileEvent::Finished {
            name: name.to_owned(),
            generation,
        }
    }

    fn rotated(name: &str, generation: u64, skipped_days: bool) -> FileEvent {
        FileEvent::Rotated {
            name: name.to_owned(),
            generation,
            skipped_days,
        }
    }

    fn reason(tailer: &Tailer, source: &MemorySource) -> Option<RebuildReason> {
        tailer
            .rebuild_reason(source, &source.list().expect("memory listing"))
            .expect("memory prefix read")
    }

    #[test]
    fn recognizes_case_insensitive_names_and_distinguishes_numbered_files() {
        let date = LogDate {
            year: 2026,
            month: 11,
            day: 27,
        };
        assert_eq!(
            parse_log_name("viveTrackerServer_20261127.TXT"),
            Some(LogName {
                kind: FileKind::Server,
                date,
                numbered: false,
            })
        );
        assert_eq!(
            parse_log_name(DRIVER_27),
            Some(LogName {
                kind: FileKind::Driver,
                date,
                numbered: false,
            })
        );
        assert_eq!(
            parse_log_name(NUMBERED),
            Some(LogName {
                kind: FileKind::Server,
                date,
                numbered: true,
            })
        );
        for name in [
            "ViveTrackerServer_2026112.txt",
            "ViveTrackerServer_20260229.txt",
            "ViveTrackerServer_20261127_.txt",
            "ViveTrackerServer_20261127_abc.txt",
            "ViveTrackerServer_２０２６１１２７.txt",
            "ViveTrackerDongle_20261127.txt",
            "other.txt",
        ] {
            assert_eq!(parse_log_name(name), None, "{name}");
        }
        assert!(parse_log_name("ViveTrackerServer_20280229.txt").is_some());
    }

    #[test]
    fn startup_reads_only_the_previous_calendar_day_before_the_newest() {
        let mut source = MemorySource::default();
        source.write(DAY_26, b"too old");
        source.write(DAY_27, b"previous");
        source.write(DAY_28, b"today");
        source.write(NUMBERED, b"numbered");
        let mut tailer = Tailer::new(FileKind::Server);

        assert_eq!(
            read(&mut tailer, &source, 0),
            [
                FileEvent::Numbered {
                    name: NUMBERED.into(),
                },
                bytes(DAY_27, 1, b"previous"),
                finished(DAY_27, 1),
                rotated(DAY_28, 2, false),
            ]
        );
        assert!(!tailer.caught_up());
        assert!(tailer.has_work(&source.list().expect("memory listing")));
        assert_eq!(read(&mut tailer, &source, 1), [bytes(DAY_28, 2, b"today")]);
        assert!(tailer.caught_up());
        assert!(!tailer.has_work(&source.list().expect("memory listing")));

        source.files.remove(DAY_27);
        tailer.reset();
        assert_eq!(read(&mut tailer, &source, 2), [bytes(DAY_28, 3, b"today")]);
        assert!(tailer.caught_up());
    }

    #[test]
    fn startup_finishes_on_rotation_to_an_empty_current_snapshot() {
        let mut source = MemorySource::default();
        source.write(DAY_27, b"previous");
        source.write(DAY_28, b"");
        let mut tailer = Tailer::new(FileKind::Server);
        assert_eq!(
            read(&mut tailer, &source, 0),
            [
                bytes(DAY_27, 1, b"previous"),
                finished(DAY_27, 1),
                rotated(DAY_28, 2, false)
            ]
        );
        assert!(tailer.caught_up());
        assert!(!tailer.has_work(&source.list().expect("memory listing")));
    }

    #[test]
    fn pushes_only_new_bytes_even_across_odd_utf16_boundaries() {
        let mut source = MemorySource::default();
        let encoded: Vec<_> = [0xfeff]
            .into_iter()
            .chain("first\r\nsecond\r\n".encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut tailer = Tailer::new(FileKind::Server);
        let mut decoder = Decoder::new();
        let mut output = Vec::new();
        for (index, chunk) in encoded.chunks(3).enumerate() {
            source.append(DAY_27, chunk);
            let events = read(&mut tailer, &source, index as i64);
            assert_eq!(events, [bytes(DAY_27, 1, chunk)]);
            for event in events {
                if let FileEvent::Bytes { bytes, .. } = event {
                    output.extend(decoder.push(&bytes));
                }
            }
        }
        assert_eq!(output, ["first", "second"]);
        assert!(read(&mut tailer, &source, 500).is_empty());
    }

    #[test]
    fn drains_the_old_file_before_rotation_and_counts_late_bytes_once() {
        let mut source = MemorySource::default();
        source.write(DAY_27, b"first\n");
        let mut tailer = Tailer::new(FileKind::Server);
        assert_eq!(
            read(&mut tailer, &source, 0),
            [bytes(DAY_27, 1, b"first\n")]
        );

        source.append(DAY_27, b"last");
        source.write(DAY_28, b"next\n");
        assert_eq!(
            read(&mut tailer, &source, 100),
            [
                bytes(DAY_27, 1, b"last"),
                finished(DAY_27, 1),
                rotated(DAY_28, 2, false)
            ]
        );
        source.append(DAY_27, b"late");
        assert_eq!(
            read(&mut tailer, &source, 200),
            [
                FileEvent::LateWrite {
                    name: DAY_27.into(),
                    bytes: 4
                },
                bytes(DAY_28, 2, b"next\n")
            ]
        );
        assert!(read(&mut tailer, &source, 300).is_empty());
        source.append(DAY_27, b"more");
        assert_eq!(
            read(&mut tailer, &source, 400),
            [FileEvent::LateWrite {
                name: DAY_27.into(),
                bytes: 4
            }]
        );
        source.append(DAY_27, b"outside the window");
        assert!(read(&mut tailer, &source, 30_101).is_empty());
    }

    #[test]
    fn stale_old_size_does_not_end_a_generation_before_its_final_bytes() {
        let mut source = MemorySource::default();
        source.write(DAY_27, b"first\n");
        let mut tailer = Tailer::new(FileKind::Server);
        assert_eq!(
            read(&mut tailer, &source, 0),
            [bytes(DAY_27, 1, b"first\n")]
        );

        let mut stale = source.list().expect("memory listing");
        source.append(DAY_27, &vec![b'a'; MAX_READ_BYTES]);
        source.append(DAY_27, b"last\n");
        source.write(DAY_28, b"next\n");
        stale.push(LogFile {
            name: DAY_28.into(),
            len: 5,
        });
        source.reads.borrow_mut().clear();

        let events = tailer
            .read(&source, &stale, LogTime(100))
            .expect("memory read");
        assert!(matches!(
            events.as_slice(),
            [FileEvent::Bytes { name, generation: 1, bytes }]
                if name == DAY_27 && bytes.len() == MAX_READ_BYTES
        ));
        let reads = source.reads.borrow();
        assert_eq!(
            reads.iter().filter(|(_, offset, _)| *offset != 0).count(),
            1
        );
        assert!(reads.iter().all(|(_, _, max)| *max <= MAX_READ_BYTES));
        drop(reads);
        assert!(tailer.has_work(&stale));

        assert_eq!(
            read(&mut tailer, &source, 200),
            [
                bytes(DAY_27, 1, b"last\n"),
                finished(DAY_27, 1),
                rotated(DAY_28, 2, false)
            ]
        );
        source.append(DAY_27, b"late");
        assert_eq!(
            read(&mut tailer, &source, 300),
            [
                FileEvent::LateWrite {
                    name: DAY_27.into(),
                    bytes: 4
                },
                bytes(DAY_28, 2, b"next\n")
            ]
        );
    }

    #[test]
    fn rotation_identifies_a_gap_in_calendar_dates() {
        let mut source = MemorySource::default();
        source.write(DAY_27, b"day 27");
        let mut tailer = Tailer::new(FileKind::Server);
        read(&mut tailer, &source, 0);
        source.write(DAY_29, b"day 29");
        assert_eq!(
            read(&mut tailer, &source, 500),
            [finished(DAY_27, 1), rotated(DAY_29, 2, true)]
        );
    }

    #[test]
    fn replacement_shrink_and_disappearance_are_reported_before_reading() {
        let mut source = MemorySource::default();
        source.write(DAY_26, b"previous");
        source.write(DAY_27, &vec![b'a'; PREFIX_BYTES + 2]);
        let mut tailer = Tailer::new(FileKind::Server);
        read(&mut tailer, &source, 0);
        read(&mut tailer, &source, 0);

        source.append(DAY_27, b"append");
        assert_eq!(
            read(&mut tailer, &source, 500),
            [bytes(DAY_27, 2, b"append")]
        );
        assert!(
            source.reads.borrow().iter().any(|(name, offset, max)| {
                name == DAY_27 && *offset == 0 && *max == PREFIX_BYTES
            })
        );

        source.write(DAY_27, &vec![b'b'; PREFIX_BYTES + 8]);
        assert_eq!(reason(&tailer, &source), None, "same-size replacement");
        source.append(DAY_27, b"growth");
        assert_eq!(reason(&tailer, &source), Some(RebuildReason::Replaced));

        source.write(DAY_27, b"shorter");
        assert_eq!(reason(&tailer, &source), Some(RebuildReason::Truncated));
        tailer.reset();
        assert_eq!(
            read(&mut tailer, &source, 1000),
            [
                bytes(DAY_26, 3, b"previous"),
                finished(DAY_26, 3),
                rotated(DAY_27, 4, false)
            ]
        );
        assert_eq!(
            read(&mut tailer, &source, 1000),
            [bytes(DAY_27, 4, b"shorter")]
        );

        source.files.remove(DAY_27);
        assert_eq!(reason(&tailer, &source), Some(RebuildReason::Disappeared));
        tailer.reset();
        assert_eq!(
            read(&mut tailer, &source, 1500),
            [bytes(DAY_26, 5, b"previous")]
        );
        source.files.clear();
        assert_eq!(reason(&tailer, &source), Some(RebuildReason::Disappeared));
        tailer.reset();
        assert_eq!(read(&mut tailer, &source, 2000), [FileEvent::Missing]);
        assert!(read(&mut tailer, &source, 2500).is_empty());
        source.write(DAY_28, b"returned");
        assert_eq!(reason(&tailer, &source), Some(RebuildReason::Restored));
    }

    #[test]
    fn driver_can_disappear_and_resume_without_requesting_a_rebuild() {
        let mut source = MemorySource::default();
        source.write(DRIVER_27, b"old driver");
        let mut tailer = Tailer::new(FileKind::Driver);
        assert_eq!(
            read(&mut tailer, &source, 0),
            [bytes(DRIVER_27, 1, b"old driver")]
        );
        source.files.clear();
        assert_eq!(read(&mut tailer, &source, 500), [FileEvent::Missing]);
        assert!(!tailer.found());
        assert!(tailer.caught_up());
        source.write(DRIVER_27, b"new driver");
        assert_eq!(
            read(&mut tailer, &source, 1000),
            [bytes(DRIVER_27, 2, b"new driver")]
        );
    }

    #[test]
    fn content_reads_are_bounded_and_startup_target_does_not_move_with_appends() {
        let mut source = MemorySource::default();
        source.write(DAY_27, &vec![b'a'; MAX_READ_BYTES + 1]);
        let mut tailer = Tailer::new(FileKind::Server);
        let first = read(&mut tailer, &source, 0);
        assert!(
            matches!(first.as_slice(), [FileEvent::Bytes { bytes, .. }] if bytes.len() == MAX_READ_BYTES)
        );
        assert!(!tailer.caught_up());

        source.append(DAY_27, &vec![b'b'; MAX_READ_BYTES]);
        let next = read(&mut tailer, &source, 1);
        assert!(
            matches!(next.as_slice(), [FileEvent::Bytes { bytes, .. }] if bytes.len() == MAX_READ_BYTES)
        );
        assert!(tailer.caught_up());
        assert!(tailer.has_work(&source.list().expect("memory listing")));
        assert_eq!(read(&mut tailer, &source, 2), [bytes(DAY_27, 1, b"b")]);
        assert!(
            source
                .reads
                .borrow()
                .iter()
                .all(|(_, _, max)| *max <= MAX_READ_BYTES)
        );
    }
}
