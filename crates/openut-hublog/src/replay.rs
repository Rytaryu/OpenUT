//! Read saved Hub logs through the same decoding and event mapping as live input.

use std::collections::VecDeque;

use openut_core::engine::{
    Engine, EngineConfig, EngineStats, FileKind as Source, LineQuality, Output,
};
use openut_core::event::Timed;
use openut_core::rules::Detection;
use openut_core::state::Snapshot;
use openut_core::time::{LogDate, LogTime, UtcOffset};

use crate::decode::Decoder;
use crate::header::{FileKind, FilledEntry, OffsetFill, ParsedLine, parse_line};
use crate::map::{MapCounts, map_entry};
use crate::tail::parse_log_name;

/// A saved Hub server or driver log file, already read by the caller.
pub struct ReplayFile<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReplayCounts {
    pub lines: usize,
    pub meta: usize,
    pub no_header: usize,
}

impl ReplayCounts {
    fn observe(&mut self, line: &ParsedLine<'_>) {
        self.lines += 1;
        match line {
            ParsedLine::Meta(_) => self.meta += 1,
            ParsedLine::NoHeader(_) => self.no_header += 1,
            ParsedLine::Entry(_) => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReplayResult {
    pub counts: ReplayCounts,
    pub detections: Vec<Detection>,
    pub stats: EngineStats,
    pub snapshot: Snapshot,
    /// Server rows with an unreadable or discarded offset.
    pub bad_offset: usize,
    /// Driver rows with an unreadable or discarded offset.
    pub driver_bad_offset: usize,
}

/// Count records in saved files using a fresh decoder for each file.
pub fn replay(files: &[ReplayFile<'_>]) -> ReplayCounts {
    let mut counts = ReplayCounts::default();
    for file in files {
        read_lines(file, |line| counts.observe(&line));
    }
    counts
}

/// Replay server and driver files through the watcher engine up to the last server entry.
pub fn replay_with_engine(files: &[ReplayFile<'_>], cfg: EngineConfig) -> ReplayResult {
    replay_with_flush(files, cfg, 0)
}

/// Replay the saved logs and explicitly advance the final server time by `flush_s` seconds.
pub fn replay_with_flush(
    files: &[ReplayFile<'_>],
    cfg: EngineConfig,
    flush_s: u64,
) -> ReplayResult {
    let mut input = ReplayInput::from_files(files);
    let mut engine = Engine::new(cfg);
    let mut stats = EngineStats::default();
    let mut detections = Vec::new();

    while let Some((source, record)) = input.next_record() {
        match record {
            ReplayRecord::Event(event) => engine.apply(event),
            ReplayRecord::Line(at, quality) => engine.note_line_at(source, at, quality),
            ReplayRecord::Rebuild(_) => {
                collect_segment(&mut engine, &mut detections, &mut stats);
                engine.rebuild();
            }
        }
    }
    if flush_s > 0
        && let Some(last) = engine.now()
    {
        let millis = i64::try_from(flush_s.saturating_mul(1_000)).unwrap_or(i64::MAX);
        engine.advance_to(LogTime(last.0.saturating_add(millis)));
    }
    collect_segment(&mut engine, &mut detections, &mut stats);
    stats.pushes = engine.stats().pushes;

    ReplayResult {
        counts: input.counts,
        detections,
        stats,
        snapshot: engine.snapshot(),
        bad_offset: input.offsets.bad_offset(FileKind::Server),
        driver_bad_offset: input.offsets.bad_offset(FileKind::Driver),
    }
}

fn collect_segment(engine: &mut Engine, detections: &mut Vec<Detection>, stats: &mut EngineStats) {
    detections.extend(
        engine
            .drain()
            .into_iter()
            .filter_map(|output| match output {
                Output::Detection(detection) => Some(detection),
                Output::Notice(_) => None,
            }),
    );
    stats.merge(EngineStats {
        // Engine keeps deliveries from this run across rebuilds.
        pushes: 0,
        ..engine.stats().clone()
    });
}

fn read_lines(file: &ReplayFile<'_>, mut visit: impl FnMut(ParsedLine<'_>)) {
    let mut decoder = Decoder::new();
    for chunk in file.bytes.chunks(8192) {
        for line in decoder.push(chunk) {
            visit(parse_line(&line));
        }
    }
    for line in decoder.finish() {
        visit(parse_line(&line));
    }
}

#[derive(Default)]
struct ReplayInput {
    counts: ReplayCounts,
    offsets: OffsetFill,
    server: VecDeque<ReplayRecord>,
    driver: VecDeque<ReplayRecord>,
}

impl ReplayInput {
    fn from_files(files: &[ReplayFile<'_>]) -> Self {
        let mut files: Vec<_> = files
            .iter()
            .filter_map(|file| parse_log_name(file.name).map(|name| (file, name)))
            .filter(|(_, name)| !name.numbered)
            .collect();
        files.sort_by_key(|(_, name)| (name.kind == FileKind::Driver, name.date));

        let mut input = Self::default();
        let mut previous_server_day = None;
        for (file, name) in files {
            let records = match name.kind {
                FileKind::Server => {
                    let day = name
                        .date
                        .days_since_epoch()
                        .expect("validated filename date");
                    if previous_server_day.is_some_and(|previous| day - previous > 1) {
                        input.server.push_back(ReplayRecord::Rebuild(name.date));
                    }
                    previous_server_day = Some(day);
                    &mut input.server
                }
                FileKind::Driver => {
                    // All server files were examined. If none had an entry, use the
                    // documented zero-offset basis without staging any driver rows.
                    input.offsets.finish(|_| {});
                    &mut input.driver
                }
            };
            read_lines(file, |line| {
                input.counts.observe(&line);
                match line {
                    ParsedLine::Entry(entry) => input.offsets.push(name.kind, entry, |filled| {
                        records.push_back(ReplayRecord::from(filled));
                    }),
                    ParsedLine::NoHeader(_) => {
                        records.push_back(ReplayRecord::Line(None, LineQuality::NoHeader));
                    }
                    ParsedLine::Meta(_) => {}
                }
            });
        }
        input
    }

    fn next_record(&mut self) -> Option<(Source, ReplayRecord)> {
        // Only compare the heads to preserve each stream's records and any clock regressions.
        // The server wins ties. Equal-time driver rows still arrive before any later server row
        // can move the engine past that time.
        let driver_first = self.driver.front().is_some_and(|driver| {
            self.server.front().is_none_or(|server| match server {
                ReplayRecord::Rebuild(date) => driver
                    .time()
                    .is_none_or(|(at, offset)| at.to_local(offset).0 < *date),
                _ => driver.time().map(|(at, _)| at) < server.time().map(|(at, _)| at),
            })
        });
        if driver_first {
            self.driver
                .pop_front()
                .map(|record| (Source::Driver, record))
        } else {
            self.server
                .pop_front()
                .map(|record| (Source::Server, record))
        }
    }
}

enum ReplayRecord {
    Event(Timed),
    Line(Option<(LogTime, UtcOffset)>, LineQuality),
    Rebuild(LogDate),
}

impl ReplayRecord {
    fn time(&self) -> Option<(LogTime, UtcOffset)> {
        match self {
            Self::Event(event) => Some((event.at, event.offset)),
            Self::Line(at, _) => *at,
            Self::Rebuild(_) => None,
        }
    }
}

impl From<FilledEntry<'_>> for ReplayRecord {
    fn from(filled: FilledEntry<'_>) -> Self {
        let mut counts = MapCounts::default();
        if let Some(event) = map_entry(&filled.entry, filled.file, &mut counts) {
            return Self::Event(event);
        }
        let quality = if counts.server.unparsed + counts.driver.unparsed > 0 {
            LineQuality::Unparsed
        } else {
            LineQuality::Other
        };
        Self::Line(Some((filled.entry.at, filled.entry.offset)), quality)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openut_core::rules::{RuleId, Step};

    const SERVER: &str = "ViveTrackerServer_20261128.txt";
    const DRIVER: &str = "ViveTrackerDriver_20261128.txt";

    fn file<'a>(name: &'a str, source: &'a str) -> ReplayFile<'a> {
        ReplayFile {
            name,
            bytes: source.as_bytes(),
        }
    }

    #[test]
    fn replays_utf16_crlf_records_with_metadata_and_embedded_lf() {
        let source = concat!(
            "# Log begin at 2026-11-28 00:00:53.613\r\n",
            "[2026-11-28 00:00:53.613 +9 0x1 INFO ] [1] Log Start\r\n",
            "[2026-11-28 00:00:54.000 0x1 INFO Test] [1] ",
            "Save map creation time: Wed Nov 25 21:00:00 2026\n.\r\n",
            "[2026-11-28 00:00:55.000 +9 0x1 ERROR Test] [1] \r\n",
            "broken record\r\n",
        );
        let bytes: Vec<_> = [0xfeff]
            .into_iter()
            .chain(source.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            replay(&[ReplayFile {
                name: "ViveTrackerServer_20261128.txt",
                bytes: &bytes,
            }]),
            ReplayCounts {
                lines: 5,
                meta: 1,
                no_header: 1,
            },
        );
    }

    #[test]
    fn counts_unterminated_lines_and_resets_decoder_between_files() {
        let header = "[2026-11-28 12:00:00.000 +9 0x1 INFO Test] hello";
        let later = format!("{header}\nunparsed");
        let earlier: Vec<_> = [0xfeff]
            .into_iter()
            .chain(header.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        let files = [
            ReplayFile {
                name: "ViveTrackerServer_20261128.txt",
                bytes: later.as_bytes(),
            },
            ReplayFile {
                name: "ViveTrackerServer_20261127.txt",
                bytes: &earlier,
            },
        ];
        assert_eq!(
            replay(&files),
            ReplayCounts {
                lines: 3,
                meta: 0,
                no_header: 1,
            }
        );
    }

    #[test]
    fn replays_driver_metadata_and_timeout_with_embedded_lf() {
        let driver = concat!(
            "# Log begin at 2026-11-28 00:00:53.613\r\n",
            "# WriteTimeInterval=5000\r\n",
            "[2026-11-28 00:00:54.000 +9 0x1 INFO TrackerDongleHID::DataDispatchThreadProc] [1] ",
            "Wait for HID data got timeout.\n\r\n",
        );
        assert_eq!(
            replay(&[ReplayFile {
                name: "ViveTrackerDriver_20261128.txt",
                bytes: driver.as_bytes(),
            }]),
            ReplayCounts {
                lines: 3,
                meta: 2,
                no_header: 0,
            },
        );
    }

    #[test]
    fn repairs_missing_and_malformed_server_offsets_before_advancing_time() {
        for offset in ["", "+5.5 "] {
            let server = format!(
                "{}[2026-11-28 14:00:01.000 {offset}0x1 INFO Test] [1] still waiting\n{}",
                concat!(
                    "[2026-11-28 13:59:49.000 +9 0x1 INFO Test] [1] Add new tracker: (2), connection type: 1\n",
                    "[2026-11-28 13:59:50.000 +9 0x1 INFO Test] [1] Client tracker (index: 2) is connected to host: 1\n",
                    "[2026-11-28 14:00:00.000 +9 0x1 INFO Test] [1] still waiting\n",
                ),
                "[2026-11-28 14:00:02.000 +9 0x1 INFO Test] [1] still waiting\n",
            );
            let driver = "[2026-11-28 15:00:00.000 +9 0x1 INFO Test] [1] driver activity\n";
            let files = [file(DRIVER, driver), file(SERVER, &server)];
            let result = replay_with_engine(&files, EngineConfig::default());
            assert_eq!(result.counts, replay(&files));
            assert_eq!(result.stats.server.lines, 5);
            assert_eq!(result.stats.driver.lines, 1);
            assert_eq!(result.stats.time_regressions, 0);
            assert_eq!(result.bad_offset, 1);
            assert_eq!(result.snapshot.at.to_string(), "2026-11-28 05:00:02.000");
            assert!(result.detections.is_empty());

            let exactly_due = replay_with_flush(&files, EngineConfig::default(), 18);
            assert!(exactly_due.detections.is_empty());
            let past_due = replay_with_flush(&files, EngineConfig::default(), 19);
            let [detection] = &past_due.detections[..] else {
                panic!("expected one R2 detection after the deadline");
            };
            assert_eq!(detection.rule, RuleId::R2);
            assert_eq!(detection.step, Step::Raised);
            assert_eq!(detection.at.to_string(), "2026-11-28 05:00:20.000");
            assert_eq!(
                past_due.stats,
                EngineStats {
                    pushes: 1,
                    ..result.stats
                }
            );
        }
    }

    #[test]
    fn uses_the_first_server_offset_to_match_a_same_time_driver_notice() {
        for (server_offset, driver_offset, bad_server, bad_driver) in
            [("+9 ", "", 0, 1), ("", "+9 ", 3, 2)]
        {
            let server = format!(
                concat!(
                    "[2026-11-28 14:00:00.000 {0}0x1 INFO Test] [1] Add new tracker: (2), connection type: 1\n",
                    "[2026-11-28 14:00:40.000 +9 0x1 INFO Test] [1] Remove tracker (2), connection type: 1, power state:11\n",
                    "[2026-11-28 14:00:46.000 +9 0x1 INFO Test] [1] server activity\n",
                ),
                server_offset
            );
            let driver = format!(
                concat!(
                    "[2026-11-28 14:00:40.000 {0}0x1 INFO Test] [1] (2) Tracker is going to power-off.\n",
                    "[2026-11-28 14:00:47.000 +9 0x1 INFO Test] [1] driver activity\n",
                ),
                driver_offset
            );
            let files = [file(DRIVER, &driver), file(SERVER, &server)];
            let result = replay_with_engine(&files, EngineConfig::default());
            assert!(result.detections.is_empty());
            assert_eq!(result.stats.power_offs, 1);
            assert_eq!(result.stats.time_regressions, 0);
            assert_eq!(result.bad_offset, bad_server);
            assert_eq!(result.driver_bad_offset, bad_driver);
        }
    }

    #[test]
    fn preserves_clock_regressions_and_counts_non_event_lines() {
        let server = concat!(
            "# header\n",
            "[2026-11-28 14:00:01.000 +9 0x1 INFO Test] [1] server activity\n",
            "[2026-11-28 14:00:00.000 +9 0x1 INFO Test] [1] Add new tracker: (invalid), connection type: 1\n",
            "broken record\n",
        );
        let result = replay_with_engine(&[file(SERVER, server)], EngineConfig::default());
        assert_eq!(result.stats.time_regressions, 1);
        assert_eq!(result.stats.server.other, 1);
        assert_eq!(result.stats.server.unparsed, 1);
        assert_eq!(result.stats.server.no_header, 1);
        assert_eq!(
            result.counts,
            ReplayCounts {
                lines: 4,
                meta: 1,
                no_header: 1
            }
        );
    }

    #[test]
    fn keeps_detections_but_rebuilds_pending_state_when_server_files_skip_a_day() {
        let earlier = concat!(
            "[2026-11-26 14:00:00.000 +9 0x1 INFO Test] [1] Add new tracker: (1), connection type: 1\n",
            "[2026-11-26 14:00:01.000 +9 0x1 INFO Test] [1] Client tracker (index: 1) is connected to host: 1\n",
            "[2026-11-26 14:00:32.000 +9 0x1 INFO Test] [1] Add new tracker: (2), connection type: 1\n",
            "[2026-11-26 14:00:33.000 +9 0x1 INFO Test] [1] Client tracker (index: 2) is connected to host: 1\n",
        );
        let later = "[2026-11-28 14:00:00.000 0x1 INFO Test] [1] server activity\n";
        let result = replay_with_engine(
            &[
                file(SERVER, later),
                file("ViveTrackerServer_20261126.txt", earlier),
            ],
            EngineConfig::default(),
        );
        let [detection] = &result.detections[..] else {
            panic!("expected only the detection raised before the gap");
        };
        assert_eq!(detection.rule, RuleId::R2);
        assert_eq!(detection.at.to_string(), "2026-11-26 05:00:31.000");
        assert!(result.snapshot.trackers.is_empty());
        assert_eq!(result.snapshot.at.to_string(), "2026-11-28 05:00:00.000");
        assert_eq!(result.stats.server.lines, 5);
        assert_eq!(result.stats.pushes, 1);
    }
}
