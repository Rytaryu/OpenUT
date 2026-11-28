use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use openut::config::WatchConfig;
use openut::source_fs::FsLogSource;
use openut::summary_io::{SummaryWriter, WriteOutcome};
use openut::watcher::{SummaryWrite, Watcher};
use openut_core::engine::Output;
use openut_core::rules::Detection;
use openut_core::summary::Summary;
use openut_core::time::{LogDate, LogTime, UtcOffset};
use openut_hublog::replay::{ReplayFile, replay_with_flush};

const PREVIOUS_NAME: &str = "ViveTrackerServer_20261127.txt";
const CURRENT_NAME: &str = "ViveTrackerServer_20261128.txt";
const PREVIOUS: &str = include_str!("replay/synthetic-midnight/ViveTrackerServer_20261127.txt");
const CURRENT: &str = include_str!("replay/synthetic-midnight/ViveTrackerServer_20261128.txt");

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("openut-watch-daily-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create watch test folder: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn time(day: u8, hour: u8, minute: u8) -> LogTime {
    LogTime::from_local(
        LogDate {
            year: 2026,
            month: 11,
            day,
        },
        hour,
        minute,
        0,
        0,
        UtcOffset(9 * 60),
    )
    .unwrap()
}

fn append(path: &Path, bytes: &str) {
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(bytes.as_bytes())
        .unwrap();
}

fn poll(
    watcher: &mut Watcher,
    source: &FsLogSource,
    writer: &SummaryWriter,
    at: LogTime,
    detections: &mut Vec<Detection>,
) -> Vec<(Summary, WriteOutcome)> {
    let mut written = Vec::new();
    for _ in 0..10 {
        let round = watcher.poll(source, at, None);
        detections.extend(round.outputs.into_iter().filter_map(|output| match output {
            Output::Detection(detection) => Some(detection),
            _ => None,
        }));
        for write in round.summaries {
            let SummaryWrite::Daily(summary) = write else {
                panic!("an append or normal rotation triggered a rebuild");
            };
            let outcome = writer.write(&summary).unwrap();
            written.push((summary, outcome));
        }
        if !round.more {
            assert!(
                !round.catching_up,
                "small fixture did not finish catch-up at {at}"
            );
            return written;
        }
    }
    panic!("small fixture was not fully read at {at}");
}

#[test]
fn ongoing_watch_and_startup_match_replay_and_keep_the_existing_final_file() {
    let directory = TestDirectory::new();
    let logs = directory.0.join("hub");
    fs::create_dir(&logs).unwrap();
    let output = directory.0.join("summary");
    let writer = SummaryWriter::new(&output);
    let source = FsLogSource::new(&logs);
    let previous_log = logs.join(PREVIOUS_NAME);
    let current_log = logs.join(CURRENT_NAME);
    let previous_json = output.join("2026-11-27.json");
    let current_json = output.join("2026-11-28.json");
    let config = WatchConfig::default();
    let replay = replay_with_flush(
        &[
            ReplayFile {
                name: PREVIOUS_NAME,
                bytes: PREVIOUS.as_bytes(),
            },
            ReplayFile {
                name: CURRENT_NAME,
                bytes: CURRENT.as_bytes(),
            },
        ],
        config.engine.clone(),
        600,
    );
    let [closed, open] = replay.summaries.as_slice() else {
        panic!("the fixture must produce two daily summaries");
    };
    assert!(closed.is_final);
    assert!(!open.is_final);
    assert_eq!(open.trackers[0].tracking_s, 1770.0);

    let mut ongoing = Watcher::new(config.clone());
    let mut ongoing_detections = Vec::new();
    let (initial, tail) = PREVIOUS.split_at(PREVIOUS.find("[2026-11-27 23:59:").unwrap());
    fs::write(&previous_log, initial).unwrap();
    poll(
        &mut ongoing,
        &source,
        &writer,
        time(27, 23, 50),
        &mut ongoing_detections,
    );
    append(&previous_log, tail);
    poll(
        &mut ongoing,
        &source,
        &writer,
        time(27, 23, 59),
        &mut ongoing_detections,
    );
    let (initial, tail) = CURRENT.split_at(CURRENT.find("[2026-11-28 00:04:30.").unwrap());
    fs::write(&current_log, initial).unwrap();
    poll(
        &mut ongoing,
        &source,
        &writer,
        time(28, 0, 4),
        &mut ongoing_detections,
    );
    append(&current_log, tail);
    let finalized = poll(
        &mut ongoing,
        &source,
        &writer,
        time(28, 0, 30),
        &mut ongoing_detections,
    );
    assert!(finalized.contains(&(closed.clone(), WriteOutcome::Written)));
    poll(
        &mut ongoing,
        &source,
        &writer,
        time(28, 0, 40),
        &mut ongoing_detections,
    );
    assert_eq!(
        ongoing.open_summaries().as_slice(),
        std::slice::from_ref(open)
    );
    assert_eq!(ongoing_detections, replay.detections);
    let saved_previous = fs::read(&previous_json).unwrap();
    let saved_current = fs::read(&current_json).unwrap();
    for (saved, summary) in [(&saved_previous, closed), (&saved_current, open)] {
        let actual: serde_json::Value = serde_json::from_slice(saved).unwrap();
        let expected: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(summary).unwrap()).unwrap();
        assert_eq!(actual, expected);
    }

    let mut restarted = Watcher::new(config);
    let reopened_writer = SummaryWriter::new(&output);
    let mut restarted_detections = Vec::new();
    let caught_up = poll(
        &mut restarted,
        &source,
        &reopened_writer,
        time(28, 0, 40),
        &mut restarted_detections,
    );
    assert!(caught_up.contains(&(closed.clone(), WriteOutcome::AlreadyFinal)));
    assert_eq!(
        restarted.open_summaries().as_slice(),
        std::slice::from_ref(open)
    );
    assert_eq!(restarted_detections, replay.detections);
    assert_eq!(fs::read(previous_json).unwrap(), saved_previous);
    assert_eq!(fs::read(current_json).unwrap(), saved_current);
}
