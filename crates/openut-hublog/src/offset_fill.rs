//! Shared time basis for parsed server and driver records (watcher design §4.3).

use std::collections::VecDeque;

use openut_core::time::{LogTime, UtcOffset};

use crate::header::Entry;

const MAX_PENDING_DRIVER_ENTRIES: usize = 20_000;
const MILLIS_PER_MINUTE: i64 = 60_000;

/// The file stream that a record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Server,
    Driver,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FilledEntry<'a> {
    pub file: FileKind,
    pub entry: Entry<'a>,
    /// Whether the offset was unreadable or had to be discarded.
    pub bad_offset: bool,
}

#[derive(Debug, Clone, Copy)]
enum Basis {
    Fixed(UtcOffset),
    NoOffset,
}

#[derive(Debug, Default)]
struct Stream {
    previous_offset: Option<UtcOffset>,
    bad_offset: usize,
}

/// Interprets both streams against the basis from the first server entry.
///
/// Keep one instance across log rotations. Supply entries from each stream in
/// file order. Use a new instance when rebuilding from the beginning.
#[derive(Debug, Default)]
pub struct OffsetFill {
    basis: Option<Basis>,
    server: Stream,
    driver: Stream,
    pending_driver: VecDeque<BufferedEntry>,
}

impl OffsetFill {
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits a corrected entry. Early driver entries wait for the first server
    /// entry and are emitted in order just before it.
    pub fn push(
        &mut self,
        file: FileKind,
        entry: Entry<'_>,
        mut emit: impl FnMut(FilledEntry<'_>),
    ) {
        let basis = match self.basis {
            Some(basis) => basis,
            None if file == FileKind::Driver => {
                self.buffer_driver(entry);
                return;
            }
            None => {
                let basis = if entry.offset_known {
                    Basis::Fixed(entry.offset)
                } else {
                    Basis::NoOffset
                };
                self.basis = Some(basis);
                self.flush_driver(basis, &mut emit);
                basis
            }
        };

        let stream = match file {
            FileKind::Server => &mut self.server,
            FileKind::Driver => &mut self.driver,
        };
        emit(stream.fill(basis, file, entry));
    }

    /// Finishes a replay, using offset zero if no server entry was found.
    pub fn finish(&mut self, mut emit: impl FnMut(FilledEntry<'_>)) {
        let basis = *self.basis.get_or_insert(Basis::NoOffset);
        self.flush_driver(basis, &mut emit);
    }

    /// Counts emitted entries with unusable offsets and evicted driver entries.
    pub fn bad_offset(&self, file: FileKind) -> usize {
        match file {
            FileKind::Server => self.server.bad_offset,
            FileKind::Driver => self.driver.bad_offset,
        }
    }

    fn buffer_driver(&mut self, entry: Entry<'_>) {
        if self.pending_driver.len() == MAX_PENDING_DRIVER_ENTRIES
            && let Some(dropped) = self.pending_driver.pop_front()
        {
            self.driver.bad_offset += 1;
            // Retain the stream's last known offset even after evicting its text.
            if dropped.offset_known {
                self.driver.previous_offset = Some(dropped.offset);
            }
        }
        self.pending_driver.push_back(BufferedEntry::from(entry));
    }

    fn flush_driver(&mut self, basis: Basis, emit: &mut impl FnMut(FilledEntry<'_>)) {
        for pending in self.pending_driver.drain(..) {
            emit(self.driver.fill(basis, FileKind::Driver, pending.entry()));
        }
    }
}

impl Stream {
    fn fill<'a>(&mut self, basis: Basis, file: FileKind, mut entry: Entry<'a>) -> FilledEntry<'a> {
        let (offset, bad_offset) = match basis {
            Basis::NoOffset => (UtcOffset(0), true),
            Basis::Fixed(_) if entry.offset_known => {
                self.previous_offset = Some(entry.offset);
                (entry.offset, false)
            }
            Basis::Fixed(initial) => (self.previous_offset.unwrap_or(initial), true),
        };

        // The parsed time already incorporates the original offset.
        let adjustment = (i64::from(entry.offset.0) - i64::from(offset.0)) * MILLIS_PER_MINUTE;
        entry.at = LogTime(entry.at.0.saturating_add(adjustment));
        entry.offset = offset;
        self.bad_offset += usize::from(bad_offset);
        FilledEntry {
            file,
            entry,
            bad_offset,
        }
    }
}

// Only early driver entries need to own their text; the other entries are
// passed to the callback with the original borrowed text.
#[derive(Debug)]
struct BufferedEntry {
    at: LogTime,
    offset: UtcOffset,
    offset_known: bool,
    level: Box<str>,
    func: Box<str>,
    thread: Option<Box<str>>,
    body: Box<str>,
}

impl From<Entry<'_>> for BufferedEntry {
    fn from(entry: Entry<'_>) -> Self {
        Self {
            at: entry.at,
            offset: entry.offset,
            offset_known: entry.offset_known,
            level: entry.level.into(),
            func: entry.func.into(),
            thread: entry.thread.map(Into::into),
            body: entry.body.into(),
        }
    }
}

impl BufferedEntry {
    fn entry(&self) -> Entry<'_> {
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

#[cfg(test)]
mod tests {
    use crate::header::{ParsedLine, parse_line};

    use super::*;
    use FileKind::{Driver, Server};

    fn record(second: u8, offset: &str, body: &str) -> String {
        format!("[2026-11-27 14:00:{second:02}.000 {offset} 0x1 INFO f] [1] {body}")
    }

    fn parse_entry(line: &str) -> Entry<'_> {
        let ParsedLine::Entry(entry) = parse_line(line) else {
            panic!("expected a parsed entry");
        };
        entry
    }

    fn push_one(
        fill: &mut OffsetFill,
        file: FileKind,
        second: u8,
        offset: &str,
    ) -> (String, UtcOffset, bool) {
        let mut output = None;
        let line = record(second, offset, "line");
        fill.push(file, parse_entry(&line), |filled| {
            assert_eq!(filled.file, file);
            assert!(output.is_none());
            output = Some((
                filled.entry.at.to_string(),
                filled.entry.offset,
                filled.bad_offset,
            ));
        });
        output.expect("one entry is ready")
    }

    fn assert_time(actual: (String, UtcOffset, bool), utc: &str, minutes: i16, bad_offset: bool) {
        assert_eq!(
            actual,
            (
                format!("2026-11-27 {utc}.000"),
                UtcOffset(minutes),
                bad_offset
            )
        );
    }

    #[test]
    fn fills_a_missing_or_invalid_offset_between_known_offsets() {
        for unknown in ["", "+5.5"] {
            let mut fill = OffsetFill::new();
            assert_time(push_one(&mut fill, Server, 0, "+9"), "05:00:00", 540, false);
            assert_time(
                push_one(&mut fill, Server, 1, unknown),
                "05:00:01",
                540,
                true,
            );
            assert_time(push_one(&mut fill, Server, 2, "+9"), "05:00:02", 540, false);
            assert_eq!(fill.bad_offset(Server), 1);
            assert_eq!(fill.bad_offset(Driver), 0);
        }
    }

    #[test]
    fn unknown_first_server_offset_makes_both_streams_use_zero() {
        let mut fill = OffsetFill::new();
        let server_first = push_one(&mut fill, Server, 0, "");
        let driver_first = push_one(&mut fill, Driver, 0, "+9");
        assert_eq!(server_first, driver_first);
        assert_time(server_first, "14:00:00", 0, true);
        assert_time(push_one(&mut fill, Server, 1, "+9"), "14:00:01", 0, true);
        assert_time(push_one(&mut fill, Driver, 1, "+9"), "14:00:01", 0, true);
        assert_eq!(fill.bad_offset(Server), 2);
        assert_eq!(fill.bad_offset(Driver), 2);
    }

    #[test]
    fn previous_known_offsets_are_updated_independently_per_stream() {
        let mut fill = OffsetFill::new();
        assert_time(push_one(&mut fill, Server, 0, "+9"), "05:00:00", 540, false);
        assert_time(
            push_one(&mut fill, Server, 1, "+10"),
            "04:00:01",
            600,
            false,
        );
        assert_time(push_one(&mut fill, Driver, 1, "+8"), "06:00:01", 480, false);
        assert_time(push_one(&mut fill, Server, 2, ""), "04:00:02", 600, true);
        assert_time(push_one(&mut fill, Driver, 2, ""), "06:00:02", 480, true);
        assert_eq!(fill.bad_offset(Server), 1);
        assert_eq!(fill.bad_offset(Driver), 1);
    }

    #[test]
    fn a_new_fill_starts_with_no_basis_or_previous_offsets() {
        let mut fill = OffsetFill::new();
        let _ = push_one(&mut fill, Server, 0, "+10");
        let _ = push_one(&mut fill, Driver, 0, "");
        assert_eq!(fill.bad_offset(Driver), 1);

        let mut rebuilt = OffsetFill::new();
        rebuilt.push(Driver, parse_entry(&record(0, "", "driver")), |_| {
            panic!("driver must wait until the server establishes the basis");
        });
        assert_eq!(rebuilt.bad_offset(Driver), 0);
        rebuilt.push(Server, parse_entry(&record(0, "+9", "server")), |filled| {
            assert_eq!(filled.entry.offset, UtcOffset(540));
        });
        assert_eq!(rebuilt.bad_offset(Driver), 1);
        assert_eq!(rebuilt.bad_offset(Server), 0);
    }

    #[test]
    fn missing_first_driver_offset_uses_the_initial_server_offset() {
        let mut fill = OffsetFill::new();
        let server_first = push_one(&mut fill, Server, 0, "+9");
        let _ = push_one(&mut fill, Server, 1, "+10");
        let driver_first = push_one(&mut fill, Driver, 0, "");
        assert_eq!(server_first.0, driver_first.0);
        assert_time(driver_first, "05:00:00", 540, true);
        assert_time(push_one(&mut fill, Driver, 1, "+9"), "05:00:01", 540, false);
    }

    #[test]
    fn waiting_driver_records_emit_in_input_order_on_first_server_entry() {
        let mut fill = OffsetFill::new();
        for (body, offset) in [("first", ""), ("second", "+8"), ("third", "")] {
            let line = record(0, offset, body);
            fill.push(Driver, parse_entry(&line), |_| {
                panic!("driver still waiting")
            });
        }
        let mut output = Vec::new();
        fill.push(Server, parse_entry(&record(0, "+9", "server")), |filled| {
            assert_eq!(filled.entry.level, "INFO");
            assert_eq!(filled.entry.func, "f");
            assert_eq!(filled.entry.thread, Some("1"));
            output.push((
                filled.file,
                filled.entry.body.to_owned(),
                filled.entry.offset.0,
            ));
        });
        let output: Vec<_> = output
            .iter()
            .map(|(file, body, offset)| (*file, body.as_str(), *offset))
            .collect();
        assert_eq!(
            output,
            [
                (Driver, "first", 540),
                (Driver, "second", 480),
                (Driver, "third", 480),
                (Server, "server", 540),
            ]
        );
        assert_eq!(fill.bad_offset(Driver), 2);
    }

    #[test]
    fn waiting_driver_queue_evicts_its_oldest_record_at_the_limit() {
        let mut fill = OffsetFill::new();
        let oldest = record(0, "+8", "oldest");
        let queued = record(1, "", "queued");
        let newest = record(2, "+7", "newest");
        for line in std::iter::once(&oldest)
            .chain(std::iter::repeat_n(&queued, MAX_PENDING_DRIVER_ENTRIES - 1))
            .chain(std::iter::once(&newest))
        {
            fill.push(Driver, parse_entry(line), |_| {
                panic!("driver still waiting")
            });
        }
        assert_eq!(fill.bad_offset(Driver), 1);

        let mut retained = 0;
        fill.push(Server, parse_entry(&record(2, "+9", "server")), |filled| {
            if filled.file == Driver {
                retained += 1;
                if retained < MAX_PENDING_DRIVER_ENTRIES {
                    assert_eq!(filled.entry.body, "queued");
                    assert_eq!(filled.entry.at.to_string(), "2026-11-27 06:00:01.000");
                    assert_eq!(filled.entry.offset, UtcOffset(480));
                } else {
                    assert_eq!(filled.entry.body, "newest");
                }
            }
        });
        assert_eq!(retained, MAX_PENDING_DRIVER_ENTRIES);
        assert_eq!(fill.bad_offset(Driver), MAX_PENDING_DRIVER_ENTRIES);
        assert_eq!(fill.bad_offset(Server), 0);
    }

    #[test]
    fn finishing_without_server_flushes_driver_entries_at_offset_zero() {
        let mut fill = OffsetFill::new();
        for (second, offset) in [(0, "+9"), (1, "")] {
            fill.push(
                Driver,
                parse_entry(&record(second, offset, "driver")),
                |_| {
                    panic!("driver still waiting");
                },
            );
        }
        let mut output = Vec::new();
        fill.finish(|filled| {
            assert_eq!(filled.file, Driver);
            assert_eq!(filled.entry.offset, UtcOffset(0));
            assert!(filled.bad_offset);
            output.push(filled.entry.at.to_string());
        });
        assert_eq!(
            output,
            ["2026-11-27 14:00:00.000", "2026-11-27 14:00:01.000"]
        );
        assert_eq!(fill.bad_offset(Driver), 2);
        assert_eq!(fill.bad_offset(Server), 0);
        fill.finish(|_| panic!("already flushed"));
    }

    #[test]
    fn timestamps_and_offsets_are_independent_of_stream_interleaving() {
        let run = |order: [usize; 6], initial_offset: &str| {
            let input = [
                (Server, record(0, initial_offset, "server 0")),
                (Server, record(1, "+10", "server 1")),
                (Server, record(2, "", "server 2")),
                (Driver, record(0, "", "driver 0")),
                (Driver, record(1, "-5:30", "driver 1")),
                (Driver, record(2, "+5.5", "driver 2")),
            ];
            let mut fill = OffsetFill::new();
            let mut output = Vec::new();
            for index in order {
                let (file, line) = &input[index];
                fill.push(*file, parse_entry(line), |filled| {
                    output.push((
                        filled.entry.body.to_owned(),
                        filled.entry.at,
                        filled.entry.offset,
                        filled.bad_offset,
                    ));
                });
            }
            output.sort_by(|a, b| a.0.cmp(&b.0));
            assert_eq!(output.len(), input.len());
            (output, fill.bad_offset(Server), fill.bad_offset(Driver))
        };

        for initial_offset in ["+9", "", "+0"] {
            let server_first = run([0, 1, 2, 3, 4, 5], initial_offset);
            assert_eq!(server_first, run([3, 4, 5, 0, 1, 2], initial_offset));
            assert_eq!(server_first, run([0, 3, 1, 4, 2, 5], initial_offset));
        }
    }
}
