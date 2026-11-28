use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use openut_dongle::record::{
    Device, EndReason, Header, Mode, Note, RawReport, Record, RecordReader, RecordWriter,
    TrackerLabel,
};
use openut_dongle::replay::Replay;
use openut_dongle::run::{
    Clock, Notice, ReadOutcome, ReportSource, RunOptions, RunResult, SourceItem, Stamp, main_loop,
    reader_loop, run_probe, run_record,
};
use openut_dongle::tracker::{Link, Transition};

const BASE_WALL_MS: i64 = 10_000;

fn wall_ms(mono_us: u64) -> i64 {
    BASE_WALL_MS + i64::try_from(mono_us / 1_000).expect("synthetic time fits in an i64")
}

fn header(mode: Mode) -> Header {
    Header {
        tool: "synthetic-live-test".into(),
        mode,
        started_wall_ms: BASE_WALL_MS,
        device: Device {
            vid: 0,
            pid: 0,
            interface: 0,
            usage_page: 0,
            usage: 0,
        },
        server_running_at_start: mode == Mode::ProbeSharing,
        anonymized: false,
    }
}

fn pose(tracker: u8, sequence: u16) -> Vec<u8> {
    let mut bytes = vec![0x28];
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&[0, tracker, 0, 0, 0, 0, 0x10, 0x01, 37]);
    let mut payload = [0; 37];
    payload[0] = tracker;
    payload[14..16].copy_from_slice(&0x3c00_u16.to_le_bytes());
    payload[36] = 2;
    bytes.extend_from_slice(&payload);
    bytes
}

fn rows(recording: &[u8]) -> Vec<Record> {
    let mut reader = RecordReader::new(Cursor::new(recording)).expect("read synthetic header");
    let mut rows = Vec::new();
    while let Some(row) = reader.read_record().expect("read synthetic row") {
        rows.push(row);
    }
    assert_eq!(reader.stats().time_reversed, 0);
    assert_eq!(reader.stats().malformed, 0);
    rows
}

fn replay_transitions(recording: &[u8]) -> Vec<Transition> {
    let mut replay = Replay::new(Cursor::new(recording)).expect("open replay");
    let mut transitions = Vec::new();
    while let Some(step) = replay.read_step().expect("replay synthetic row") {
        transitions.extend(step.transitions);
    }
    assert_eq!(replay.stats().time_reversed, 0);
    transitions
}

fn assert_ended(recording: &[u8], at_us: u64, note: Note, reason: EndReason) {
    let rows = rows(recording);
    assert_eq!(
        rows[rows.len() - 2],
        Record::Note {
            mono_us: at_us,
            wall_ms: wall_ms(at_us),
            text: note,
        }
    );
    assert!(matches!(
        rows.last(),
        Some(Record::Footer {
            mono_us,
            wall_ms: Some(wall),
            reason: end,
            time_reversed: 0,
            ..
        }) if (*mono_us, *wall, *end) == (at_us, wall_ms(at_us), reason)
    ));
}

fn stamp(mono_us: u64) -> Stamp {
    Stamp {
        mono_us,
        wall_ms: wall_ms(mono_us),
    }
}

struct FakeClock(Arc<AtomicU64>);

impl Clock for FakeClock {
    fn now(&mut self) -> Stamp {
        stamp(self.0.load(Ordering::Relaxed))
    }
}

struct FakeSource {
    time: Arc<AtomicU64>,
    reads: VecDeque<(u64, ReadOutcome)>,
}

impl ReportSource for FakeSource {
    fn read_timeout(&mut self, timeout: Duration) -> ReadOutcome {
        assert_eq!(timeout, Duration::from_millis(100));
        let Some((at_us, read)) = self.reads.pop_front() else {
            return ReadOutcome::Removed;
        };
        self.time.store(at_us, Ordering::Relaxed);
        read
    }
}

fn script(reads: impl IntoIterator<Item = (u64, ReadOutcome)>) -> (FakeSource, FakeClock) {
    let time = Arc::new(AtomicU64::new(0));
    (
        FakeSource {
            time: Arc::clone(&time),
            reads: reads.into_iter().collect(),
        },
        FakeClock(time),
    )
}

fn record_with(
    reads: impl IntoIterator<Item = (u64, ReadOutcome)>,
    duration: Duration,
    guard: impl FnMut() -> bool,
) -> Vec<u8> {
    let mut writer = RecordWriter::new(Vec::new(), &header(Mode::Record)).expect("write header");
    let (source, clock) = script(reads);
    run_record(
        source,
        clock,
        &mut writer,
        stamp(0),
        duration,
        guard,
        |_| Ok(()),
    )
    .expect("write synthetic recording");
    writer.into_inner()
}

#[test]
fn record_writes_a_note_and_footer_when_the_duration_ends() {
    let recording = record_with(
        [
            (1_000_000, ReadOutcome::Idle),
            (2_000_000, ReadOutcome::Idle),
        ],
        Duration::from_secs(2),
        || false,
    );
    assert_ended(&recording, 2_000_000, Note::Duration, EndReason::Duration);
}

#[test]
fn record_writes_a_note_and_footer_when_the_server_appears() {
    let mut checks = 0;
    let recording = record_with(
        [
            (1_000_000, ReadOutcome::Idle),
            (2_000_000, ReadOutcome::Idle),
            (3_000_000, ReadOutcome::Idle),
            (4_000_000, ReadOutcome::Idle),
        ],
        Duration::from_secs(6),
        || {
            checks += 1;
            checks == 2
        },
    );
    assert_eq!(checks, 2);
    assert_ended(
        &recording,
        4_000_000,
        Note::ServerAppeared,
        EndReason::ServerAppeared,
    );
}

#[test]
fn record_writes_the_timestamp_and_code_of_a_read_error() {
    let recording = record_with(
        [
            (100_000, ReadOutcome::Idle),
            (550_000, ReadOutcome::ReadError(21)),
        ],
        Duration::from_secs(2),
        || false,
    );
    assert_ended(
        &recording,
        550_000,
        Note::ReadError(21),
        EndReason::ReadError,
    );
}

#[test]
fn a_stalled_reader_checks_the_server_without_advancing_tracker_time() {
    let (tx, rx) = mpsc::channel();
    tx.send(SourceItem::Report(RawReport {
        mono_us: 10_000_000,
        wall_ms: wall_ms(10_000_000),
        bytes: pose(1, 1),
    }))
    .expect("queue the first report");
    // Keep the producer open with no more items to distinguish a stall from disconnection.
    let mut writer = RecordWriter::new(Vec::new(), &header(Mode::Record)).expect("write header");
    let mut checks = 0;
    let mut stalls = 0;
    let result = main_loop(
        &rx,
        &AtomicBool::new(false),
        &mut writer,
        RunOptions {
            start: stamp(0),
            duration: Duration::from_secs(20),
            mode: Mode::Record,
        },
        || {
            checks += 1;
            checks == 2
        },
        |notice| {
            if let Notice::Stalled = notice {
                stalls += 1;
            }
            Ok(())
        },
    )
    .expect("finish the stalled capture");
    drop(tx);

    assert_eq!(stalls, 1);
    assert_eq!(result.table.now_us(), 10_000_000);
    assert_eq!(
        result
            .table
            .view(TrackerLabel('A'))
            .expect("tracker A")
            .link,
        Link::Receiving
    );
    assert_ended(
        &writer.into_inner(),
        10_000_000,
        Note::ServerAppeared,
        EndReason::ServerAppeared,
    );
}

fn collect_items(reads: impl IntoIterator<Item = (u64, ReadOutcome)>) -> Vec<SourceItem> {
    let (source, clock) = script(reads);
    let mut items = Vec::new();
    reader_loop(
        source,
        clock,
        |item| {
            items.push(item);
            true
        },
        &AtomicBool::new(false),
    );
    items
}

fn run_main(
    stream: &mpsc::Receiver<SourceItem>,
    stop: &AtomicBool,
    duration: Duration,
) -> (Vec<u8>, Vec<Transition>, RunResult) {
    let mut writer = RecordWriter::new(Vec::new(), &header(Mode::Record)).expect("write header");
    let mut transitions = Vec::new();
    let state = main_loop(
        stream,
        stop,
        &mut writer,
        RunOptions {
            start: stamp(0),
            duration,
            mode: Mode::Record,
        },
        || false,
        |notice| {
            if let Notice::Transition(change) = notice {
                transitions.push(change);
            }
            Ok(())
        },
    )
    .expect("consume recorded stream");
    (writer.into_inner(), transitions, state)
}

fn run_queued(items: Vec<SourceItem>, duration: Duration) -> (Vec<u8>, Vec<Transition>, RunResult) {
    let (tx, rx) = mpsc::channel();
    for item in items {
        tx.send(item).expect("enqueue synthetic item");
    }
    drop(tx);
    run_main(&rx, &AtomicBool::new(false), duration)
}

fn ticks(recording: &[u8]) -> Vec<u64> {
    rows(recording)
        .iter()
        .filter_map(|row| match row {
            Record::Tick { mono_us, .. } => Some(*mono_us),
            _ => None,
        })
        .collect()
}

#[test]
fn idle_polls_keep_ticks_moving_and_expire_all_trackers_at_the_same_deadline() {
    let items = collect_items(
        (1..=3)
            .map(|tracker| {
                (
                    10_000_000,
                    ReadOutcome::Report(pose(tracker, u16::from(tracker))),
                )
            })
            .chain((1..=30).map(|poll| (10_000_000 + poll * 100_000, ReadOutcome::Idle))),
    );
    assert_eq!(
        items
            .iter()
            .filter_map(|item| match item {
                SourceItem::Idle(at) => Some(at.mono_us),
                _ => None,
            })
            .collect::<Vec<_>>(),
        (1..=30)
            .map(|poll| 10_000_000 + poll * 100_000)
            .collect::<Vec<_>>()
    );
    let (recording, transitions, result) = run_queued(items, Duration::from_secs(13));
    assert_eq!(
        ticks(&recording),
        [10_000_000, 11_000_000, 12_000_000, 13_000_000]
    );
    assert_eq!(result.reason, EndReason::Duration);
    let silent_changes: Vec<_> = transitions
        .iter()
        .filter(|change| change.to == Link::Silent)
        .copied()
        .collect();
    assert_eq!(
        silent_changes,
        ['A', 'B', 'C'].map(|label| Transition {
            at_us: 12_000_000,
            tracker: TrackerLabel(label),
            from: Link::Receiving,
            to: Link::Silent,
        })
    );
    for tracker in result.table.views() {
        assert_eq!(tracker.link, Link::Silent);
        assert_eq!(tracker.pose_rate_hz, 0.0);
    }
    assert_eq!(replay_transitions(&recording), transitions);
}

#[test]
fn processing_a_backlog_matches_per_item_processing_and_replay() {
    let reads = || {
        [
            (10_000_000, ReadOutcome::Report(pose(1, 1))),
            (11_990_000, ReadOutcome::Report(pose(1, 2))),
            (12_100_000, ReadOutcome::Idle),
        ]
    };
    let duration = Duration::from_millis(12_100);
    let (queued, queued_changes, queued_result) = run_queued(collect_items(reads()), duration);
    let (per_item, per_item_changes, per_item_result) = thread::scope(|scope| {
        let (tx, rx) = mpsc::sync_channel(0);
        let (source, clock) = script(reads());
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let reader = scope.spawn(move || {
            reader_loop(source, clock, |item| tx.send(item).is_ok(), &reader_stop);
        });
        let result = run_main(&rx, &stop, duration);
        drop(rx);
        reader.join().expect("join unbuffered reader");
        result
    });

    assert_eq!(queued, per_item);
    assert_eq!(ticks(&queued), [10_000_000, 11_990_000]);
    assert_eq!(queued_changes, per_item_changes);
    assert_eq!(queued_changes, replay_transitions(&queued));
    assert_eq!(
        queued_changes,
        [Transition {
            at_us: 10_000_000,
            tracker: TrackerLabel('A'),
            from: Link::NotSeen,
            to: Link::Receiving,
        }]
    );
    assert_eq!(queued_result.time_reversed, 0);
    assert_eq!(per_item_result.time_reversed, 0);
}

#[test]
fn probe_opens_only_after_baseline_and_records_both_windows_in_order() {
    let duration = Duration::from_secs(2);
    let (source, clock) = script([
        (2_100_000, ReadOutcome::Report(pose(1, 1))),
        (3_000_000, ReadOutcome::Idle),
        (4_000_000, ReadOutcome::Idle),
    ]);
    let now = Arc::clone(&clock.0);
    let mut writer = RecordWriter::new(Vec::new(), &header(Mode::ProbeSharing)).expect("header");
    run_probe(
        &mut writer,
        clock,
        duration,
        |wait| {
            assert_eq!(wait, duration);
            assert_eq!(now.load(Ordering::Relaxed), 0);
            now.store(2_000_000, Ordering::Relaxed);
        },
        || {
            assert_eq!(now.load(Ordering::Relaxed), 2_000_000);
            Ok(source)
        },
        |_| Ok(()),
    )
    .expect("write synthetic probe record");

    let recorded = rows(&writer.into_inner());
    let mut report_times = Vec::new();
    let mut windows = Vec::new();
    for row in &recorded {
        match row {
            Record::Input(report) => report_times.push(report.mono_us),
            Record::Note { mono_us, text, .. }
                if matches!(
                    text,
                    Note::WindowBaseStart
                        | Note::WindowBaseEnd
                        | Note::WindowProbeStart
                        | Note::WindowProbeEnd
                ) =>
            {
                windows.push((*mono_us, *text));
            }
            _ => {}
        }
    }
    assert_eq!(report_times, [2_100_000]);
    assert_eq!(
        windows,
        [
            (0, Note::WindowBaseStart),
            (2_000_000, Note::WindowBaseEnd),
            (2_000_000, Note::WindowProbeStart),
            (4_000_000, Note::WindowProbeEnd),
        ]
    );
    assert!(
        recorded
            .windows(2)
            .all(|pair| pair[0].mono_us() <= pair[1].mono_us())
    );
}
