use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use openut::replay::{Expectations, ObservedSummaries, RaisedDetection};
use openut_hublog::decode::decode_utf16le_text;
use openut_hublog::header::{ParsedLine, parse_line};
use openut_hublog::replay::{ReplayCounts, ReplayFile, replay};

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
    ($($test:ident => $case:literal, $lines:literal, $meta:literal);+ $(;)?) => {
        const CASES: &[(&str, &str)] = &[$(($case, stringify!($test))),+];
        $(
            #[test]
            fn $test() {
                let case = SavedCase::load($case);
                assert_eq!(
                    replay(&case.inputs()),
                    ReplayCounts { lines: $lines, meta: $meta, no_header: 0 },
                );
            }
        )+
    };
}

replay_cases! {
    synthetic_r1_second_host_command => "synthetic-r1-second-host-command", 132, 14;
    synthetic_normal_start => "synthetic-normal-start", 28, 14;
    synthetic_utf16 => "synthetic-utf16", 28, 14;
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
fn utf16_matches_utf8_contents_and_record_counts() {
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
    assert_eq!(replay(&utf16.inputs()), replay(&utf8.inputs()));
}

#[test]
fn r1_expectation_matches_the_saved_host_and_warning_sequence() {
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
    let (index, raised) = warnings
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.at.0 - first.0 >= 15_000)
        .expect("warning sequence lasts more than 15 seconds");
    assert_eq!(index, 28);
    case.expectations
        .compare(
            &[RaisedDetection {
                rule: "R1",
                tracker: hosts.last().copied(),
                at: raised.at,
                offset: raised.offset,
            }],
            &ObservedSummaries::new(),
        )
        .expect("expected R1 host and time match the fixture");
}

fn check_detections(name: &str) {
    let case = SavedCase::load(name);
    let (detections, summaries) = replay_with_engine(&case.inputs(), &case.expectations);
    if let Err(report) = case.expectations.compare(&detections, &summaries) {
        panic!("{name}\n{report}");
    }
}

fn replay_with_engine(
    _inputs: &[ReplayFile<'_>],
    _expectations: &Expectations,
) -> (Vec<RaisedDetection<'static>>, ObservedSummaries) {
    // Wire to the real engine before enabling these tests. Empty output would
    // let the normal-start cases pass without exercising any detection rules.
    unimplemented!("replay detections require the M1-6 engine")
}

#[test]
#[ignore = "M1-6 で有効にする"]
fn synthetic_r1_second_host_command_detections() {
    check_detections("synthetic-r1-second-host-command");
}

#[test]
#[ignore = "M1-6 で有効にする"]
fn synthetic_normal_start_detections() {
    check_detections("synthetic-normal-start");
}

#[test]
#[ignore = "M1-6 で有効にする"]
fn synthetic_utf16_detections() {
    check_detections("synthetic-utf16");
}
