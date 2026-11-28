use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use openut::replay::{Expectations, ObservedSummaries, RaisedDetection};
use openut_core::engine::{Engine, EngineConfig, FileKind as Source, LineQuality, Output};
use openut_core::rules::{RuleId, Step};
use openut_core::summary::Summary;
use openut_core::time::{LogDate, LogTime, UtcOffset};
use openut_core::tracker::TrackerKey;
use openut_hublog::decode::decode_utf16le_text;
use openut_hublog::header::{FileKind, ParsedLine, parse_line};
use openut_hublog::map::{MapCounts, map_entry};
use openut_hublog::replay::{ReplayCounts, ReplayFile, ReplayResult, replay, replay_with_flush};
use openut_hublog::tail::parse_log_name;

struct SavedCase {
    files: Vec<(String, Vec<u8>)>,
    expectations: Expectations,
}

impl SavedCase {
    fn load(name: &str) -> Self {
        let directory = fixture_root().join(name);
        let files = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("could not open replay case {name}: {error}"))
            .map(|entry| entry.expect("fixture directory entries should be readable"))
            .filter_map(|entry| {
                let file_name = entry.file_name().into_string().ok()?;
                if !is_log_file_name(&file_name) {
                    return None;
                }
                let bytes = fs::read(entry.path())
                    .unwrap_or_else(|error| panic!("could not open {name}/{file_name}: {error}"));
                Some((file_name, bytes))
            })
            .collect();
        let source = fs::read_to_string(directory.join("expect.toml"))
            .unwrap_or_else(|error| panic!("could not open {name}/expect.toml: {error}"));
        let expectations = Expectations::parse(&source)
            .unwrap_or_else(|error| panic!("invalid {name}/expect.toml: {error}"));
        Self {
            files,
            expectations,
        }
    }

    fn inputs(&self) -> Vec<ReplayFile<'_>> {
        self.files
            .iter()
            .map(|(name, bytes)| ReplayFile { name, bytes })
            .collect()
    }

    fn run_replay(&self, inputs: &[ReplayFile<'_>]) -> ReplayResult {
        replay_with_flush(
            inputs,
            EngineConfig::default(),
            u64::from(self.expectations.replay.flush_s),
        )
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replay")
}

fn is_log_file_name(name: &str) -> bool {
    name.strip_prefix("ViveTrackerServer_")
        .or_else(|| name.strip_prefix("ViveTrackerDriver_"))
        .and_then(|name| name.strip_suffix(".txt"))
        .is_some_and(|date| date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()))
}

macro_rules! replay_cases {
    ($($test:ident => $case:literal, $lines:literal, $meta:literal $(, $check:ident)?);+ $(;)?) => {
        const CASES: &[(&str, &str)] = &[$(($case, stringify!($test))),+];
        $(
            #[test]
            fn $test() {
                let case = SavedCase::load($case);
                assert_eq!(
                    replay(&case.inputs()),
                    ReplayCounts { lines: $lines, meta: $meta, no_header: 0 },
                );
                $($check(&case);)?
            }
        )+
    };
}

replay_cases! {
    synthetic_r1_second_host_command => "synthetic-r1-second-host-command", 132, 14, check_replay;
    synthetic_normal_start => "synthetic-normal-start", 28, 14, check_replay;
    synthetic_utf16 => "synthetic-utf16", 28, 14, check_replay;
    synthetic_r1_gap_then_fault => "synthetic-r1-gap-then-fault", 140, 14, check_replay;
    synthetic_r4_power_off => "synthetic-r4-power-off", 40, 28, check_power_off_detections;
    synthetic_midnight => "synthetic-midnight", 27, 15, check_midnight;
    synthetic_midnight_no_prev => "synthetic-midnight-no-prev", 6, 1, check_midnight;
    hub_2026_11_23_28 => "hub-2026-11-23-28", 91_356, 317;
}

#[test]
fn every_fixture_has_a_matching_test_function() {
    let expected: BTreeSet<_> = CASES
        .iter()
        .map(|&(name, test)| {
            assert_eq!(test, name.replace('-', "_"));
            name.to_owned()
        })
        .collect();
    let directories: BTreeSet<_> = fs::read_dir(fixture_root())
        .expect("fixture root should be readable")
        .map(|entry| entry.expect("fixture directory entries should be readable"))
        .filter(|entry| {
            entry
                .file_type()
                .expect("fixture type should be readable")
                .is_dir()
        })
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .expect("case names must be UTF-8")
        })
        .collect();
    assert_eq!(directories, expected);
}

#[test]
fn utf16_matches_utf8_contents_and_replay_output() {
    let utf16 = SavedCase::load("synthetic-utf16");
    let utf8 = SavedCase::load("synthetic-normal-start");
    let [(_, encoded)] = utf16.files.as_slice() else {
        panic!("expected a single UTF-16 fixture file");
    };
    let [(_, plain)] = utf8.files.as_slice() else {
        panic!("expected a single UTF-8 fixture file");
    };
    let text = std::str::from_utf8(plain).expect("normal-start fixture is UTF-8");
    // A Windows checkout can already have CRLF in the text fixture.
    let expected = text.replace("\r\n", "\n").replace('\n', "\r\n");
    assert_eq!(
        decode_utf16le_text(encoded).expect("UTF-16 fixture must have a LE BOM"),
        expected,
    );
    assert_eq!(
        utf16.run_replay(&utf16.inputs()),
        utf8.run_replay(&utf8.inputs()),
    );
}

#[test]
fn r1_fixture_has_two_host_commands_and_550_ms_warning_intervals() {
    let case = SavedCase::load("synthetic-r1-second-host-command");
    let [(_, bytes)] = case.files.as_slice() else {
        panic!("expected a single R1 fixture file");
    };
    let log = std::str::from_utf8(bytes).expect("R1 fixture is UTF-8");
    let entries: Vec<_> = log
        .lines()
        .filter_map(|line| match parse_line(line) {
            ParsedLine::Entry(entry) => Some(entry),
            _ => None,
        })
        .collect();
    let hosts: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .body
                .strip_prefix("Tracker (")?
                .strip_suffix(") - switch to HOST mode.")?
                .parse::<u16>()
                .ok()
        })
        .collect();
    assert_eq!(hosts, [1, 1]);

    let warnings: Vec<_> = entries
        .iter()
        .filter(|entry| entry.body == "Wifi info is not yet available.")
        .collect();
    assert_eq!(warnings.len(), 110);
    assert!(warnings.iter().all(|entry| entry.level == "WARN"));
    assert!(
        warnings
            .windows(2)
            .all(|pair| pair[1].at.0 - pair[0].at.0 == 550)
    );
    let first = warnings[0].at;
    let index = warnings
        .iter()
        .position(|entry| entry.at.0 - first.0 >= 15_000)
        .expect("warning sequence lasts more than 15 seconds");
    assert_eq!(index, 28);
}

fn check_replay(case: &SavedCase) -> ReplayResult {
    let result = case.run_replay(&case.inputs());
    let raised: Vec<_> = result
        .detections
        .iter()
        .filter(|detection| detection.step == Step::Raised)
        .map(|detection| RaisedDetection {
            rule: detection.rule.as_str(),
            tracker: detection.tracker.map(|tracker| tracker.0),
            at: detection.at,
            // Each synthetic fixture uses a consistent +09:00 throughout.
            offset: UtcOffset(9 * 60),
        })
        .collect();
    let summaries: ObservedSummaries = result
        .summaries
        .iter()
        .map(|summary| {
            let table = toml::Table::try_from(summary).expect("summary serializes as a TOML table");
            (summary.date.clone(), table)
        })
        .collect();
    assert_eq!(
        summaries.len(),
        result.summaries.len(),
        "duplicate summary dates"
    );
    if let Err(report) = case.expectations.compare(&raised, &summaries) {
        panic!("{report}");
    }
    result
}

fn check_power_off_detections(case: &SavedCase) {
    let result = check_replay(case);
    assert_eq!(result.stats.power_offs, 1);

    let server_files: Vec<_> = case
        .inputs()
        .into_iter()
        .filter(|file| file.name.starts_with("ViveTrackerServer_"))
        .collect();
    let without_driver = case.run_replay(&server_files);
    assert_eq!(without_driver.stats.power_offs, 0);
    let [summary] = without_driver.summaries.as_slice() else {
        panic!("expected one day of summary results");
    };
    assert!(!summary.source.driver.found);
    assert_eq!(summary.trackers[0].power_offs, 0);
    let raised: Vec<_> = without_driver
        .detections
        .iter()
        .filter(|detection| detection.step == Step::Raised)
        .map(|detection| (detection.rule, detection.tracker))
        .collect();
    assert_eq!(
        raised,
        [
            (RuleId::R4, Some(TrackerKey(1))),
            (RuleId::R4, Some(TrackerKey(2))),
        ],
    );
}

fn check_midnight(case: &SavedCase) {
    let result = check_replay(case);
    assert_eq!(result.summaries.len(), case.files.len());
}

fn apply_midnight_file(engine: &mut Engine, input: &ReplayFile<'_>) {
    let file = parse_log_name(input.name).expect("known log name");
    let source = match file.kind {
        FileKind::Server => Source::Server,
        FileKind::Driver => {
            engine
                .summary_source(Some(file.date))
                .expect("fixture day is still open")
                .driver
                .found = true;
            Source::Driver
        }
    };
    let log = std::str::from_utf8(input.bytes).expect("midnight fixture is UTF-8");
    for line in log.lines() {
        match parse_line(line) {
            ParsedLine::Meta(_) => {
                let counts = engine
                    .summary_source(Some(file.date))
                    .expect("fixture day is still open");
                match source {
                    Source::Server => counts.meta += 1,
                    Source::Driver => counts.driver.meta += 1,
                }
            }
            ParsedLine::Entry(entry) => {
                let mut counts = MapCounts::default();
                if let Some(event) = map_entry(&entry, file.kind, &mut counts) {
                    engine.apply(event);
                } else {
                    assert_eq!(counts.server.unparsed + counts.driver.unparsed, 0);
                    engine.note_line_at(source, Some((entry.at, entry.offset)), LineQuality::Other);
                }
            }
            ParsedLine::NoHeader(_) => panic!("all midnight fixture rows are readable"),
        }
    }
}

fn drain_summaries(engine: &mut Engine) -> Vec<Summary> {
    engine
        .drain()
        .into_iter()
        .filter_map(|output| match output {
            Output::DayClosed(summary) => Some(*summary),
            Output::Detection(_) | Output::Notice(_) => None,
        })
        .collect()
}

#[test]
fn steady_watch_and_rebuilding_reproduce_both_days_of_the_midnight_replay() {
    let case = SavedCase::load("synthetic-midnight");
    let mut inputs = case.inputs();
    inputs.sort_by_key(|file| file.name);
    let [previous, next] = inputs.as_slice() else {
        panic!("midnight fixture covers two days");
    };
    let expected = case.run_replay(&inputs);
    let mut engine = Engine::new(EngineConfig::default());
    for rebuilt in [false, true] {
        if rebuilt {
            engine.rebuild();
        }
        apply_midnight_file(&mut engine, previous);
        if !rebuilt {
            let now = engine.now().unwrap();
            engine.advance_to(LogTime(now.0 + 120_000));
            let days = engine.open_summaries();
            assert_eq!(days.len(), 1);
            assert!(!days[0].is_final);
        }
        apply_midnight_file(&mut engine, next);
        let mut summaries = drain_summaries(&mut engine);
        summaries.extend(engine.open_summaries());
        assert_eq!(summaries, expected.summaries, "rebuilt = {rebuilt}");
    }
}

const NEXT_DAY_LOG: &str = "ViveTrackerServer_20261128.txt";
const PREVIOUS_DAY_LOG: &str = "ViveTrackerServer_20261127.txt";

fn grace_cutoff() -> LogTime {
    let date = LogDate {
        year: 2026,
        month: 11,
        day: 28,
    };
    LogTime::from_local(date, 0, 10, 0, 0, UtcOffset(9 * 60)).unwrap()
}

fn watch_and_replay_at_eof(files: &[ReplayFile<'_>]) -> ReplayResult {
    let mut watch = Engine::new(EngineConfig::default());
    for file in files {
        apply_midnight_file(&mut watch, file);
    }
    let last_observed = watch.now().unwrap();
    let cutoff = grace_cutoff();
    watch.advance_to(cutoff);
    assert!(drain_summaries(&mut watch).is_empty());
    watch.advance_to(LogTime(cutoff.0 + 1));
    let mut summaries = drain_summaries(&mut watch);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].date, "2026-11-27");
    assert!(summaries[0].is_final);
    summaries.extend(watch.open_summaries());

    // Replay observes the same rows and reaches EOF earlier than the wall clock.
    let replay = replay_with_flush(files, EngineConfig::default(), 0);
    assert_eq!(replay.summaries, summaries);
    assert_eq!(replay.snapshot.at, last_observed);
    replay
}

#[test]
fn a_power_off_decided_after_midnight_is_counted_on_the_cut_day() {
    let previous = concat!(
        "[2026-11-27 23:58:00.000 +9 0x1 INFO OETServer::AddTrackerCB] [1] Add new tracker: (1), connection type: 1\n",
        "[2026-11-27 23:59:59.500 +9 0x1 INFO OETServer::RemoveTrackerCB] [1] Remove tracker (1), connection type: 1, power state:11\n",
    );
    let driver = "[2026-11-27 23:59:59.500 +9 0x1 INFO TrackerDongleHID::ParseIndication] [1] (1) Tracker is going to power-off.\n";
    let early = "[2026-11-28 00:00:00.100 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n";
    let late = "[2026-11-28 00:00:05.000 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n";
    for next in [late.to_owned(), format!("{early}{late}")] {
        let result = watch_and_replay_at_eof(&[
            ReplayFile {
                name: PREVIOUS_DAY_LOG,
                bytes: previous.as_bytes(),
            },
            ReplayFile {
                name: "ViveTrackerDriver_20261127.txt",
                bytes: driver.as_bytes(),
            },
            ReplayFile {
                name: NEXT_DAY_LOG,
                bytes: next.as_bytes(),
            },
        ]);
        assert_eq!(result.stats.power_offs, 1);
        assert_eq!(result.summaries[0].trackers[0].power_offs, 1);
        assert!(
            result.summaries[1]
                .trackers
                .iter()
                .all(|tracker| tracker.power_offs == 0)
        );
    }
}

#[test]
fn a_serial_learned_after_midnight_labels_the_previous_days_connection() {
    let previous = "[2026-11-27 23:59:59.000 +9 0x1 INFO OETServer::AddTrackerCB] [1] Add new tracker: (1), connection type: 1\n";
    let early = "[2026-11-28 00:00:00.100 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n";
    let serial = "[2026-11-28 00:00:00.200 +9 0x1 INFO OETServer::GetDeviceInfoAndResponse] [1] Response - cmd type: 214, cmd key: (AAAA), device index: 1, result: 0, sn: (00XX000000009)\n";
    for next in [serial.to_owned(), format!("{early}{serial}")] {
        let result = watch_and_replay_at_eof(&[
            ReplayFile {
                name: PREVIOUS_DAY_LOG,
                bytes: previous.as_bytes(),
            },
            ReplayFile {
                name: NEXT_DAY_LOG,
                bytes: next.as_bytes(),
            },
        ]);
        let [tracker] = result.summaries[0].trackers.as_slice() else {
            panic!("the entire previous connection belongs to one summary row");
        };
        assert_eq!(tracker.serial.as_deref(), Some("00XX000000009"));
        assert_eq!(tracker.connected_s, 1.0);
    }

    let at_cutoff = serial.replace("00:00:00.200", "00:10:00.000");
    let next = format!("{early}{at_cutoff}");
    let files = [
        ReplayFile {
            name: PREVIOUS_DAY_LOG,
            bytes: previous.as_bytes(),
        },
        ReplayFile {
            name: NEXT_DAY_LOG,
            bytes: next.as_bytes(),
        },
    ];
    let result = replay_with_flush(&files, EngineConfig::default(), 0);
    let mut watch = Engine::new(EngineConfig::default());
    apply_midnight_file(&mut watch, &files[0]);
    apply_midnight_file(
        &mut watch,
        &ReplayFile {
            name: NEXT_DAY_LOG,
            bytes: early.as_bytes(),
        },
    );
    watch.advance_to(grace_cutoff());
    assert!(drain_summaries(&mut watch).is_empty());
    apply_midnight_file(
        &mut watch,
        &ReplayFile {
            name: NEXT_DAY_LOG,
            bytes: at_cutoff.as_bytes(),
        },
    );
    let mut summaries = drain_summaries(&mut watch);
    assert_eq!(summaries.len(), 1);
    summaries.extend(watch.open_summaries());
    assert_eq!(result.summaries, summaries);

    let [previous, current] = result.summaries.as_slice() else {
        panic!("expected the finalized previous day and the open current day");
    };
    assert!(previous.is_final);
    assert_eq!(previous.trackers[0].serial, None);
    assert_eq!(previous.trackers[0].connected_s, 1.0);
    assert!(!current.is_final);
    assert_eq!(current.trackers[0].serial.as_deref(), Some("00XX000000009"));
}

fn edit_next_day(case: &mut SavedCase, edit: impl FnOnce(&str) -> String) {
    let (_, bytes) = case
        .files
        .iter_mut()
        .find(|(name, _)| name == NEXT_DAY_LOG)
        .expect("midnight fixture has a next-day server file");
    *bytes = edit(std::str::from_utf8(bytes).expect("midnight fixture is UTF-8")).into_bytes();
}

#[test]
fn long_gap_after_midnight_is_unobserved_with_or_without_a_previous_file() {
    for (name, connected, in_use, tracked, unobserved, losses) in [
        ("synthetic-midnight", 270.0, 270.0, 240.0, 1530.0, 1),
        ("synthetic-midnight-no-prev", 30.0, 0.0, 0.0, 1770.0, 0),
    ] {
        let mut case = SavedCase::load(name);
        let original = case.run_replay(&case.inputs());
        edit_next_day(&mut case, |source| {
            source
                .split_inclusive('\n')
                .filter(|line| {
                    !line.starts_with("[2026-11-28 00:12:")
                        && !line.starts_with("[2026-11-28 00:21:")
                })
                .collect()
        });
        let replay = case.run_replay(&case.inputs());
        let (last, prior) = replay
            .summaries
            .split_last()
            .expect("day must be summarized");
        assert_eq!(prior, &original.summaries[..original.summaries.len() - 1]);
        assert_eq!(last.date, "2026-11-28");
        assert_eq!(last.observed_until, "2026-11-28 00:30:00.000");
        assert_eq!(last.unobserved_s, unobserved, "{name}");
        let [tracker] = last.trackers.as_slice() else {
            panic!("{name}: expected one tracker summary");
        };
        assert_eq!(tracker.connected_s, connected, "{name}");
        assert_eq!(tracker.in_use_s, in_use, "{name}");
        assert_eq!(tracker.tracking_s, tracked, "{name}");
        assert_eq!(tracker.losses, losses, "{name}");
    }
}

#[test]
fn long_gap_across_midnight_splits_unobserved_time_at_midnight() {
    let mut case = SavedCase::load("synthetic-midnight");
    let original = case.run_replay(&case.inputs());
    edit_next_day(&mut case, |source| {
        source
            .replace("00:04:00.000", "00:10:00.000")
            .replace("00:04:30.000", "00:10:30.000")
    });
    let replay = case.run_replay(&case.inputs());
    let [before, after] = replay.summaries.as_slice() else {
        panic!("expected both calendar days");
    };
    assert_eq!(before.date, "2026-11-27");
    assert!(before.is_final);
    assert_eq!(before.observed_until, "2026-11-28 00:00:00.000");
    assert_eq!(
        before.unobserved_s,
        original.summaries[0].unobserved_s + 60.0
    );
    assert_eq!(before.trackers[0].tracking_s, 540.0);
    assert_eq!(after.date, "2026-11-28");
    assert_eq!(after.unobserved_s, 600.0);
    assert_eq!(after.trackers[0].connected_s, 1200.0);
    assert_eq!(after.trackers[0].tracking_s, 1170.0);
}

fn replay_server(source: &str, flush_s: u64) -> ReplayResult {
    replay_with_flush(
        &[ReplayFile {
            name: NEXT_DAY_LOG,
            bytes: source.as_bytes(),
        }],
        EngineConfig::default(),
        flush_s,
    )
}

#[test]
fn exactly_600_seconds_are_observed_but_one_more_millisecond_is_not() {
    for (last_at, tracked, unobserved) in
        [("00:10:00.000", 600.0, 0.0), ("00:10:00.001", 0.0, 600.001)]
    {
        let source = format!(
            concat!(
                "[2026-11-28 00:00:00.000 +9 0x1 INFO OETServer::AddTrackerCB] [1] Add new tracker: (1), connection type: 1\n",
                "[2026-11-28 00:00:00.000 +9 0x1 INFO Tracker::CopyFromDriverThreadProc] [1] (SteamVR: 5)(1)[12]Pose status is changed: Pose data - status: 2, pos: (0.000000, 0.000000, 0.000000)\n",
                "[2026-11-28 {} +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n",
            ),
            last_at
        );
        let replay = replay_server(&source, 0);
        let [summary] = replay.summaries.as_slice() else {
            panic!("expected one day of summary results");
        };
        assert_eq!(summary.unobserved_s, unobserved);
        assert_eq!(summary.trackers[0].tracking_s, tracked);
    }
}

#[test]
fn replay_flush_counts_new_detections_without_adding_observed_time() {
    let source = concat!(
        "[2026-11-28 12:00:00.000 +9 0x1 INFO OETServer::AddTrackerCB] [1] Add new tracker: (2), connection type: 1\n",
        "[2026-11-28 12:00:01.000 +9 0x1 INFO OETServer::ProcessDeviceMsg] [1] Client tracker (index: 2) is connected to host: 1\n",
    );
    let raw = replay_server(source, 0);
    let flushed = replay_server(source, 600);
    assert!(raw.detections.is_empty());
    assert_eq!(
        flushed
            .detections
            .iter()
            .filter(|entry| entry.rule == RuleId::R2 && entry.step == Step::Raised)
            .count(),
        1
    );
    let [before] = raw.summaries.as_slice() else {
        panic!("expected one day of raw summary results");
    };
    let [after] = flushed.summaries.as_slice() else {
        panic!("expected one day of flushed summary results");
    };
    assert_eq!(before.observed_until, "2026-11-28 12:00:01.000");
    assert_eq!(before.trackers[0].connected_s, 1.0);
    assert_eq!(after.observed_until, before.observed_until);
    assert_eq!(after.unobserved_s, before.unobserved_s);
    assert_eq!(after.trackers, before.trackers);
    assert_eq!(before.detections.r2, 0);
    assert_eq!(after.detections.r2, 1);
}

#[test]
fn a_missing_offset_stays_on_its_local_day_until_a_server_record_crosses_midnight() {
    const OLD_LOG: &str = "ViveTrackerServer_20261127.txt";
    let old = concat!(
        "[2026-11-27 23:59:58.000 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n",
        "[2026-11-27 23:59:59.000 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n",
    );
    let only_old = replay_with_flush(
        &[ReplayFile {
            name: OLD_LOG,
            bytes: old.as_bytes(),
        }],
        EngineConfig::default(),
        600,
    );
    let [before_rollover] = only_old.summaries.as_slice() else {
        panic!("no server record has crossed midnight yet");
    };
    assert!(!before_rollover.is_final);
    assert_eq!(before_rollover.observed_until, "2026-11-27 23:59:59.000");

    let crossed = format!(
        "{old}[2026-11-28 00:00:00.000 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n"
    );
    let offset_known = crossed.replace("23:59:59.000 0x1", "23:59:59.000 +9 0x1");
    let replay = |source: &str| {
        replay_with_flush(
            &[ReplayFile {
                name: OLD_LOG,
                bytes: source.as_bytes(),
            }],
            EngineConfig::default(),
            0,
        )
    };
    let actual = replay(&crossed);
    let [before, after] = actual.summaries.as_slice() else {
        panic!("a record in the old file closed its calendar day");
    };
    assert_eq!(actual.bad_offset, 1);
    assert_eq!(before.date, "2026-11-27");
    assert!(before.is_final);
    assert_eq!(before.observed_until, "2026-11-28 00:00:00.000");
    assert_eq!(before.source.bad_offset, 1);
    assert_eq!(before.source.lines, 2);
    assert_eq!(after.date, "2026-11-28");
    assert_eq!(after.source.bad_offset, 0);
    assert_eq!(after.source.lines, 1);

    let known = replay(&offset_known);
    assert_eq!(before.unobserved_s, known.summaries[0].unobserved_s);
    assert_eq!(after, &known.summaries[1]);
}

#[test]
fn driver_records_wait_for_the_server_to_observe_their_date_and_do_not_extend_time() {
    let first_day = || {
        ReplayFile {
        name: "ViveTrackerServer_20261127.txt",
        bytes: b"[2026-11-27 23:59:59.000 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n",
    }
    };
    let next_day = ReplayFile {
        name: NEXT_DAY_LOG,
        bytes: b"[2026-11-28 00:00:00.000 +9 0x1 INFO OETServer::CheckHostIsStarted] [1] Host running state:2 to 2\n",
    };
    let driver = || {
        ReplayFile {
        name: "ViveTrackerDriver_20261128.txt",
        bytes: b"[2026-11-28 00:00:01.000 +9 0x1 INFO TrackerDongleHID::DataDispatchThreadProc] [1] Wait for HID data got timeout.\n",
    }
    };
    let driver_only = replay_with_flush(&[first_day(), driver()], EngineConfig::default(), 0);
    let [before] = driver_only.summaries.as_slice() else {
        panic!("only the server's date is summarized");
    };
    assert!(!before.is_final);
    assert_eq!(before.source.driver.lines, 0);
    assert_eq!(before.observed_until, "2026-11-27 23:59:59.000");

    let replay = replay_with_flush(
        &[first_day(), next_day, driver()],
        EngineConfig::default(),
        0,
    );
    let [before, after] = replay.summaries.as_slice() else {
        panic!("the later server file opens its calendar day");
    };
    assert!(before.is_final);
    assert_eq!(before.source.driver.lines, 0);
    assert_eq!(after.date, "2026-11-28");
    assert_eq!(after.observed_until, "2026-11-28 00:00:00.000");
    assert_eq!(after.unobserved_s, 0.0);
    assert_eq!(after.source.driver.lines, 1);
    assert!(after.source.driver.found);
}
