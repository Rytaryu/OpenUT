//! Replay recorded input in file order using only the timestamps in the file.

use std::io::{self, BufRead};

use crate::decode::{Decoder, DongleEvent};
use crate::record::{Header, ReadError, ReadStats, Record, RecordReader, TrackerLabel};
use crate::tracker::{TrackerTable, Transition};

#[derive(Debug)]
pub struct ReplayStep {
    /// The row's monotonic time, clamped by the record reader if it went backwards.
    pub mono_us: u64,
    pub wall_ms: Option<i64>,
    pub event: Option<DongleEvent>,
    pub omitted_tracker: Option<TrackerLabel>,
    /// Tracker identified by the header of a malformed incoming report.
    pub malformed_tracker: Option<TrackerLabel>,
    pub transitions: Vec<Transition>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplayStats {
    pub rows: ReadStats,
    /// Incoming report rows, including reports the decoder rejects.
    pub reports: u64,
    pub malformed_reports: u64,
    pub unknown_reports: u64,
    pub omitted_reports: u64,
    /// A loss estimate is available when the recording has a footer.
    pub estimated_lost: Option<u64>,
    /// Backward timestamps found while reading or reported by the recorder.
    pub time_reversed: u64,
}

pub struct Replay<R: BufRead> {
    reader: RecordReader<R>,
    decoder: Decoder,
    table: TrackerTable,
    stats: ReplayStats,
}

impl<R: BufRead> Replay<R> {
    pub fn new(input: R) -> Result<Self, ReadError> {
        let reader = RecordReader::new(input)?;
        let decoder = Decoder::for_recording(reader.header().anonymized);
        Ok(Self {
            reader,
            decoder,
            table: TrackerTable::default(),
            stats: ReplayStats::default(),
        })
    }

    pub fn header(&self) -> &Header {
        self.reader.header()
    }

    pub fn table(&self) -> &TrackerTable {
        &self.table
    }

    pub fn stats(&self) -> ReplayStats {
        let rows = self.reader.stats();
        ReplayStats {
            rows,
            // The recorder and reader may count the same backward timestamps.
            time_reversed: self.stats.time_reversed.max(rows.time_reversed),
            ..self.stats
        }
    }

    /// EOF does not advance time beyond the last available row.
    pub fn read_step(&mut self) -> io::Result<Option<ReplayStep>> {
        let Some(record) = self.reader.read_record()? else {
            return Ok(None);
        };
        let mono_us = record.mono_us();
        let mut malformed_tracker = None;
        let (wall_ms, event, omitted_tracker, transitions) = match record {
            Record::Input(report) => {
                let event = self.decoder.decode(&report.bytes);
                self.stats.reports += 1;
                match event {
                    DongleEvent::Malformed { .. } => {
                        self.stats.malformed_reports += 1;
                        malformed_tracker = self.decoder.tracker_data_label(&report.bytes);
                    }
                    DongleEvent::Unknown { .. } | DongleEvent::OtherTrackerData { .. } => {
                        self.stats.unknown_reports += 1;
                    }
                    _ => {}
                }
                let transitions = match malformed_tracker {
                    Some(label) => self.table.receive_omitted(mono_us, label),
                    None => self.table.apply(mono_us, &event),
                };
                (Some(report.wall_ms), Some(event), None, transitions)
            }
            Record::Tick { wall_ms, .. } | Record::Note { wall_ms, .. } => {
                (Some(wall_ms), None, None, self.table.advance(mono_us))
            }
            Record::Omitted {
                wall_ms, tracker, ..
            } => {
                self.stats.omitted_reports += 1;
                let tracker = tracker.filter(|label| label.0.is_ascii_uppercase());
                let transitions = match tracker {
                    Some(label) => self.table.receive_omitted(mono_us, label),
                    None => self.table.advance(mono_us),
                };
                (Some(wall_ms), None, tracker, transitions)
            }
            Record::Footer {
                wall_ms,
                estimated_lost,
                time_reversed,
                ..
            } => {
                self.stats.estimated_lost = Some(estimated_lost);
                self.stats.time_reversed = self.stats.time_reversed.max(time_reversed);
                (wall_ms, None, None, self.table.advance(mono_us))
            }
        };
        Ok(Some(ReplayStep {
            mono_us,
            wall_ms,
            event,
            omitted_tracker,
            malformed_tracker,
            transitions,
        }))
    }
}
