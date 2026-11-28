//! Follow the server and driver logs without depending on filesystem I/O or an engine.

mod file;
mod records;
mod watermark;

#[cfg(test)]
mod tests;

use std::collections::VecDeque;

use openut_core::time::LogTime;

use crate::header::FileKind;

pub use file::{FileEvent, LogFile, LogName, LogSource, RebuildReason, Tailer, parse_log_name};
pub use records::{OwnedEntry, Record, RecordData};
pub use watermark::Watermark;

use records::RecordDecoder;

#[derive(Debug, PartialEq, Eq)]
pub enum FollowEvent {
    /// Both streams were reset for the indicated change. Discard old engine state.
    Rebuild {
        file: FileKind,
        reason: RebuildReason,
    },
    /// Both initial snapshots have been read. Buffered server records may still be waiting.
    ReadComplete,
    /// A file event other than raw bytes or decoder EOF.
    File { file: FileKind, event: FileEvent },
    Record {
        record: Record,
        /// A driver record arrived earlier than the previous driver watermark.
        late: bool,
    },
}

struct Queued {
    sequence: u64,
    event: FollowEvent,
}

/// Poll the file source, then take events one at a time using the current engine deadline.
/// The app supplies its own clock and refreshes the deadline after applying each event.
pub struct LogFollower {
    server: Tailer,
    driver: Tailer,
    decoder: RecordDecoder,
    clock: Watermark,
    lateness_ms: i64,
    server_queue: VecDeque<Queued>,
    driver_queue: VecDeque<Queued>,
    sequence: u64,
    rebuild: Option<(FileKind, RebuildReason)>,
    read_complete: bool,
    report_read_complete: bool,
    driver_needs_read: bool,
    previous_driver_time: Option<LogTime>,
}

impl Default for LogFollower {
    fn default() -> Self {
        Self::new(10_000)
    }
}

impl LogFollower {
    pub fn new(lateness_ms: i64) -> Self {
        Self {
            server: Tailer::new(FileKind::Server),
            driver: Tailer::new(FileKind::Driver),
            decoder: RecordDecoder::default(),
            clock: Watermark::new(lateness_ms),
            lateness_ms,
            server_queue: VecDeque::new(),
            driver_queue: VecDeque::new(),
            sequence: 0,
            rebuild: None,
            read_complete: false,
            report_read_complete: false,
            driver_needs_read: false,
            previous_driver_time: None,
        }
    }

    /// Reads one bounded chunk per stream from a shared file listing.
    /// Returns true when more bytes or a rotation are ready without waiting for an append.
    pub fn poll<S: LogSource>(
        &mut self,
        source: &S,
        now: LogTime,
        driver_needed_through: Option<LogTime>,
    ) -> Result<bool, S::Error> {
        let files = source.list()?;
        let server_reason = self.server.rebuild_reason(source, &files)?;
        let driver_reason = self.driver.rebuild_reason(source, &files)?;
        let rebuild = server_reason
            .map(|reason| (FileKind::Server, reason))
            .or_else(|| driver_reason.map(|reason| (FileKind::Driver, reason)));

        // Commit both cursors only after the reads succeed. An I/O error can then be retried
        // without losing a successfully read chunk from the other stream.
        let mut server = self.server.clone();
        let mut driver = self.driver.clone();
        if rebuild.is_some() {
            server.reset();
            driver.reset();
        }
        let driver_first = rebuild.is_none()
            && self.server_blocked(now, driver_needed_through)
            && driver.has_work(&files);
        let batches = if driver_first {
            let events = driver.read(source, &files, now)?;
            let server_events = if driver.has_work(&files) {
                Vec::new()
            } else {
                server.read(source, &files, now)?
            };
            [
                (FileKind::Driver, events),
                (FileKind::Server, server_events),
            ]
        } else {
            [
                (FileKind::Server, server.read(source, &files, now)?),
                (FileKind::Driver, driver.read(source, &files, now)?),
            ]
        };
        self.server = server;
        self.driver = driver;
        if let Some(rebuild) = rebuild {
            self.reset_pipeline(rebuild);
        }
        for (file, events) in batches {
            self.ingest(file, events, now);
        }

        if !self.read_complete
            && self.server.found()
            && self.server.caught_up()
            && self.driver.caught_up()
        {
            self.clock.mark_read_complete(now);
            self.read_complete = true;
            self.report_read_complete = true;
        }
        self.driver_needs_read = self.driver.has_work(&files);
        self.previous_driver_time = self.driver_watermark(now);
        Ok(self.server.has_work(&files) || self.driver_needs_read)
    }

    /// Returns the next log or file event. When a server record is blocked, driver events
    /// can pass it. Call again after applying the event with the engine's updated deadline.
    pub fn next_event(
        &mut self,
        now: LogTime,
        driver_needed_through: Option<LogTime>,
    ) -> Option<FollowEvent> {
        self.previous_driver_time = self.driver_watermark(now);
        if let Some((file, reason)) = self.rebuild.take() {
            return Some(FollowEvent::Rebuild { file, reason });
        }
        let driver = self.driver_queue.front();
        // A new driver batch can move the watermark past D. Its records must reach
        // the consumer before a server record that was waiting for that deadline.
        if driver.is_some()
            && self.driver.found()
            && driver_needed_through.is_some_and(|deadline| self.server_head_after(deadline))
        {
            return self.driver_queue.pop_front().map(|next| next.event);
        }
        let server = self
            .server_queue
            .front()
            .filter(|_| !self.server_blocked(now, driver_needed_through));
        let next = match (server, driver) {
            (Some(server), Some(driver)) if driver.sequence < server.sequence => {
                self.driver_queue.pop_front()
            }
            (Some(_), _) => self.server_queue.pop_front(),
            (None, Some(_)) => self.driver_queue.pop_front(),
            (None, None) => None,
        };
        if let Some(next) = next {
            Some(next.event)
        } else if std::mem::take(&mut self.report_read_complete) {
            Some(FollowEvent::ReadComplete)
        } else {
            None
        }
    }

    /// A safe target for idle advancement after draining all ready events. Drivers do not
    /// advance this time. A pending driver deadline caps it until the driver watermark passes.
    pub fn advance_target(
        &self,
        now: LogTime,
        driver_needed_through: Option<LogTime>,
    ) -> Option<LogTime> {
        let time = self.clock.server_time(now)?;
        Some(
            self.waiting_through(now, driver_needed_through)
                .map_or(time, |deadline| time.min(deadline)),
        )
    }

    /// Whether the driver stream is available in the latest file listing.
    pub fn driver_found(&self) -> bool {
        self.driver.found()
    }

    /// Whether all initially available bytes in both streams have been read.
    pub fn read_complete(&self) -> bool {
        self.read_complete
    }

    pub fn bad_offset(&self, file: FileKind) -> usize {
        self.decoder.bad_offset(file)
    }

    fn waiting_through(&self, now: LogTime, deadline: Option<LogTime>) -> Option<LogTime> {
        deadline.filter(|deadline| {
            self.driver.found()
                && self
                    .driver_watermark(now)
                    .is_none_or(|time| time <= *deadline)
        })
    }

    fn driver_watermark(&self, now: LogTime) -> Option<LogTime> {
        if !self.driver.found() {
            return None;
        }
        if self.driver_needs_read {
            // Do not use a wall-clock estimate to skip bytes known to be available.
            self.clock.driver_logged_time()
        } else {
            self.clock.driver_time(now)
        }
    }

    fn server_blocked(&self, now: LogTime, deadline: Option<LogTime>) -> bool {
        self.waiting_through(now, deadline)
            .is_some_and(|deadline| self.server_head_after(deadline))
    }

    fn server_head_after(&self, deadline: LogTime) -> bool {
        match self.server_queue.front().map(|item| &item.event) {
            Some(FollowEvent::Record { record, .. }) => record.at().is_some_and(|at| at > deadline),
            _ => false,
        }
    }

    fn reset_pipeline(&mut self, rebuild: (FileKind, RebuildReason)) {
        self.decoder = RecordDecoder::default();
        self.clock = Watermark::new(self.lateness_ms);
        self.server_queue.clear();
        self.driver_queue.clear();
        self.sequence = 0;
        self.rebuild = Some(rebuild);
        self.read_complete = false;
        self.report_read_complete = false;
        self.driver_needs_read = false;
        self.previous_driver_time = None;
    }

    fn ingest(&mut self, file: FileKind, events: Vec<FileEvent>, now: LogTime) {
        let mut last_server = None;
        let mut last_driver = None;
        for event in events {
            let records = match event {
                FileEvent::Bytes { bytes, .. } => self.decoder.push(file, &bytes),
                FileEvent::Finished { .. } => self.decoder.finish_file(file),
                event => {
                    if file == FileKind::Driver && matches!(event, FileEvent::Missing) {
                        self.decoder.clear_driver_file();
                        self.clock.clear_driver();
                    }
                    self.enqueue(file, FollowEvent::File { file, event });
                    continue;
                }
            };
            for record in records {
                let mut late = false;
                if let Some(at) = record.at() {
                    match record.file {
                        FileKind::Server => last_server = Some(at),
                        FileKind::Driver => {
                            late = self
                                .previous_driver_time
                                .is_some_and(|previous| at < previous);
                            last_driver = Some(at);
                        }
                    }
                }
                self.enqueue(record.file, FollowEvent::Record { record, late });
            }
        }
        if let Some(last) = last_server {
            self.clock.observe_server_batch(now, last);
        }
        if let Some(last) = last_driver {
            self.clock.observe_driver_batch(last);
        }
    }

    fn enqueue(&mut self, file: FileKind, event: FollowEvent) {
        let queue = match file {
            FileKind::Server => &mut self.server_queue,
            FileKind::Driver => &mut self.driver_queue,
        };
        queue.push_back(Queued {
            sequence: self.sequence,
            event,
        });
        self.sequence = self.sequence.saturating_add(1);
    }
}
