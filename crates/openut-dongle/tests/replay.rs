use std::io::{BufRead, Cursor};

use openut_dongle::decode::{Decoder, DongleEvent};
use openut_dongle::record::{EndReason, OmittedReason, RawReport, Record, TrackerLabel, VERSION};
use openut_dongle::replay::Replay;
use openut_dongle::tracker::{Link, TrackerTable, Transition};

const THREE_TRACKERS: &str =
    include_str!("../../../tests/fixtures/dongle/m2-3-three-trackers.jsonl");

const A: TrackerLabel = TrackerLabel('A');
const B: TrackerLabel = TrackerLabel('B');
const C: TrackerLabel = TrackerLabel('C');

fn transition(at_us: u64, tracker: TrackerLabel, from: Link, to: Link) -> Transition {
    Transition {
        at_us,
        tracker,
        from,
        to,
    }
}

fn fake_wall_ms(mono_us: u64) -> i64 {
    1_000 + i64::try_from(mono_us / 1_000).expect("test time fits in an i64")
}

fn pose_report(tracker: u8, status: u8) -> Vec<u8> {
    let mut bytes = vec![0x28, 0, 0, 0, tracker, 0, 0, 0, 0, 0x10, 0x01, 37];
    let mut pose = [0; 37];
    pose[0] = tracker;
    pose[14..16].copy_from_slice(&0x3c00_u16.to_le_bytes());
    pose[36] = status;
    bytes.extend_from_slice(&pose);
    bytes
}

fn pose(mono_us: u64, tracker: u8, status: u8) -> Record {
    Record::Input(RawReport {
        mono_us,
        wall_ms: fake_wall_ms(mono_us),
        bytes: pose_report(tracker, status),
    })
}

fn tick(mono_us: u64) -> Record {
    Record::Tick {
        mono_us,
        wall_ms: fake_wall_ms(mono_us),
    }
}

fn footer(mono_us: u64, reports: u64) -> Record {
    Record::Footer {
        mono_us,
        wall_ms: Some(fake_wall_ms(mono_us)),
        reports,
        malformed: 0,
        estimated_lost: None,
        reason: EndReason::Complete,
        time_reversed: 0,
    }
}

fn recording(records: impl IntoIterator<Item = Record>) -> Replay<Cursor<Vec<u8>>> {
    recording_with_header(records, false)
}

fn recording_with_header(
    records: impl IntoIterator<Item = Record>,
    anonymized: bool,
) -> Replay<Cursor<Vec<u8>>> {
    let header = THREE_TRACKERS.lines().next().expect("fixture header");
    let mut header: serde_json::Value =
        serde_json::from_str(header).expect("decode fixture header");
    header["version"] = serde_json::Value::from(VERSION);
    header["anonymized"] = serde_json::Value::Bool(anonymized);
    let mut input = serde_json::to_vec(&header).expect("serialize test header");
    input.push(b'\n');
    for record in records {
        serde_json::to_writer(&mut input, &record).expect("serialize synthetic row");
        input.push(b'\n');
    }
    Replay::new(Cursor::new(input)).expect("read synthetic recording")
}

fn replay_transitions<R: BufRead>(replay: &mut Replay<R>) -> Vec<Transition> {
    let mut changes = Vec::new();
    while let Some(step) = replay.read_step().expect("read replay step") {
        changes.extend(step.transitions);
    }
    changes
}

#[test]
fn replays_three_trackers_with_stable_labels_and_tracking_statuses() {
    let mut replay = Replay::new(Cursor::new(THREE_TRACKERS)).expect("open replay fixture");
    let mut trackers = Vec::new();
    let mut tracking_a = Vec::new();
    let mut changes = Vec::new();
    while let Some(step) = replay.read_step().expect("read replay step") {
        changes.extend(step.transitions);
        let table = replay.table();
        match step.event {
            Some(DongleEvent::Pair { tracker, unpaired }) => {
                trackers.push(tracker);
                assert_eq!(
                    table.view(tracker).expect("paired tracker").link,
                    if unpaired {
                        Link::Unpaired
                    } else {
                        Link::Receiving
                    }
                );
            }
            Some(DongleEvent::Pose { tracker, sample }) => {
                trackers.push(tracker);
                let view = table.view(tracker).expect("tracker with pose");
                assert_eq!(view.link, Link::Receiving);
                assert_eq!(view.last_seen_us, step.mono_us);
                assert_eq!(view.last_pose.as_ref(), Some(&sample));
                if tracker == A {
                    tracking_a.push(view.tracking_status);
                }
            }
            None => {}
            Some(event) => panic!("unexpected replay event: {event:?}"),
        }
    }
    assert_eq!(trackers, [A, A, B, C, C, B, A, B, B, A]);
    assert_eq!(tracking_a, [Some(2), Some(3), Some(2)]);
    assert_eq!(
        changes,
        [
            transition(100_000, A, Link::NotSeen, Link::Receiving),
            transition(300_000, B, Link::NotSeen, Link::Receiving),
            transition(400_000, C, Link::NotSeen, Link::Receiving),
            transition(600_000, B, Link::Receiving, Link::Unpaired),
            transition(900_000, B, Link::Unpaired, Link::Receiving),
        ]
    );
}

#[test]
fn uses_only_the_times_supplied_to_the_table() {
    let mut table = TrackerTable::default();
    let mut decoder = Decoder::default();
    let event = decoder.decode(&pose_report(1, 2));
    assert_eq!(
        table.apply(10_000_000, &event),
        [transition(10_000_000, A, Link::NotSeen, Link::Receiving)]
    );
    let view = table.view(A).expect("tracker A");
    assert_eq!(view.pose_rate_hz, 1.0);
    assert!(table.advance(11_999_999).is_empty());
    let view = table.view(A).expect("tracker A");
    assert_eq!(view.link, Link::Receiving);
    assert_eq!(view.pose_rate_hz, 0.0);
    assert_eq!(
        table.advance(13_000_000),
        [transition(12_000_000, A, Link::Receiving, Link::Silent)]
    );
}

#[test]
fn times_out_all_trackers_with_ticks_or_with_only_a_footer() {
    let stopped = || (1..=3).map(|tracker| pose(10_000_000, tracker, 2));
    let tick_rows = (11..=15).map(|at_s| tick(at_s * 1_000_000));
    let mut with_ticks = recording(stopped().chain(tick_rows));
    let mut with_footer = recording(stopped().chain([footer(15_000_000, 3)]));
    let mut no_end_marker = recording(stopped());

    let tick_changes = replay_transitions(&mut with_ticks);
    assert_eq!(tick_changes, replay_transitions(&mut with_footer));
    assert_eq!(
        &tick_changes[3..],
        [A, B, C].map(|label| transition(12_000_000, label, Link::Receiving, Link::Silent))
    );
    for replay in [&with_ticks, &with_footer] {
        assert_eq!(replay.table().now_us(), 15_000_000);
        for view in replay.table().views() {
            assert_eq!(view.link, Link::Silent);
            assert_eq!(view.pose_rate_hz, 0.0);
        }
    }

    assert_eq!(
        replay_transitions(&mut no_end_marker),
        [A, B, C].map(|label| transition(10_000_000, label, Link::NotSeen, Link::Receiving))
    );
    assert_eq!(no_end_marker.table().now_us(), 10_000_000);
}

#[test]
fn orders_the_same_deadlines_with_sparse_rows_and_100ms_advances() {
    let poses = || {
        [
            pose(9_900_000, 1, 2),
            pose(10_000_000, 2, 2),
            pose(10_200_000, 1, 2),
            pose(10_300_000, 3, 2),
        ]
    };
    let frequent_ticks = (104..=140).map(|hundred_ms| tick(hundred_ms * 100_000));
    let mut sparse = recording(poses().into_iter().chain([footer(14_000_000, 4)]));
    let mut frequent = recording(poses().into_iter().chain(frequent_ticks));
    let sparse_changes = replay_transitions(&mut sparse);
    assert_eq!(sparse_changes, replay_transitions(&mut frequent));
    assert_eq!(
        &sparse_changes[3..],
        [
            transition(12_000_000, B, Link::Receiving, Link::Silent),
            transition(12_200_000, A, Link::Receiving, Link::Silent),
            transition(12_300_000, C, Link::Receiving, Link::Silent),
        ]
    );
}

#[test]
fn reports_silence_before_a_report_after_the_deadline() {
    let mut replay = recording([pose(10_000_000, 1, 2), pose(12_100_000, 1, 2)]);
    let changes = replay_transitions(&mut replay);
    assert_eq!(
        changes,
        [
            transition(10_000_000, A, Link::NotSeen, Link::Receiving),
            transition(12_000_000, A, Link::Receiving, Link::Silent),
            transition(12_100_000, A, Link::Silent, Link::Receiving),
        ]
    );
}

#[test]
fn a_report_just_before_the_deadline_extends_it_in_file_order() {
    for intermediate_ticks in [&[][..], &[12_000_000][..], &[12_001_000, 12_099_000]] {
        let mut replay = recording(
            [pose(10_000_000, 1, 2), pose(11_990_000, 1, 2)]
                .into_iter()
                .chain(intermediate_ticks.iter().copied().map(tick))
                .chain([tick(12_100_000)]),
        );
        assert_eq!(
            replay_transitions(&mut replay),
            [transition(10_000_000, A, Link::NotSeen, Link::Receiving)]
        );
        let view = replay.table().view(A).expect("tracker A");
        assert_eq!(view.link, Link::Receiving);
        assert_eq!(view.last_seen_us, 11_990_000);
    }
}

#[test]
fn clamps_backwards_row_time_and_counts_it_once() {
    let mut replay = recording([
        pose(10_000_000, 1, 2),
        pose(9_000_000, 1, 3),
        tick(12_100_000),
    ]);
    replay.read_step().expect("read first row").expect("row");
    let adjusted = replay.read_step().expect("read second row").expect("row");
    assert_eq!(adjusted.mono_us, 10_000_000);
    let view = replay.table().view(A).expect("tracker A");
    assert_eq!(view.tracking_status, Some(3));
    assert_eq!(view.last_seen_us, 10_000_000);
    assert_eq!(
        replay_transitions(&mut replay),
        [transition(12_000_000, A, Link::Receiving, Link::Silent)]
    );
    assert_eq!(replay.stats().time_reversed, 1);
}

#[test]
fn keeps_the_labels_of_retained_and_omitted_reports_aligned() {
    let canonical_pose = |mono_us, slot, label| {
        let mut bytes = pose_report(slot, 2);
        bytes[3..9].copy_from_slice(&[0, 0xa0 | slot, 0, 0, 0, label]);
        Record::Input(RawReport {
            mono_us,
            wall_ms: fake_wall_ms(mono_us),
            bytes,
        })
    };
    let mut replay = recording_with_header(
        [
            Record::Omitted {
                mono_us: 1_000_000,
                wall_ms: fake_wall_ms(1_000_000),
                report_type: 0x28,
                len: 40,
                tracker: Some(A),
                reason: OmittedReason::TextForm,
            },
            canonical_pose(1_500_000, 1, 2),
            canonical_pose(2_000_000, 2, 1),
        ],
        true,
    );
    let omitted = replay.read_step().expect("read omitted row").expect("row");
    assert_eq!(omitted.omitted_tracker, Some(A));
    assert_eq!(
        omitted.transitions,
        [transition(1_000_000, A, Link::NotSeen, Link::Receiving)]
    );

    for expected in [B, A] {
        let step = replay.read_step().expect("read retained row").expect("row");
        assert!(matches!(
            step.event,
            Some(DongleEvent::Pose { tracker, .. }) if tracker == expected
        ));
    }
    let table = replay.table();
    assert_eq!(table.view(A).expect("tracker A").last_seen_us, 2_000_000);
    assert_eq!(table.view(B).expect("tracker B").last_seen_us, 1_500_000);
}

#[test]
fn a_conflicting_export_label_does_not_merge_two_trackers() {
    let mut canonical = pose_report(2, 3);
    canonical[3..9].copy_from_slice(&[0, 0xa2, 0, 0, 0, 1]);
    let mut replay = recording_with_header(
        [
            pose(1_000_000, 1, 2),
            Record::Input(RawReport {
                mono_us: 1_500_000,
                wall_ms: fake_wall_ms(1_500_000),
                bytes: canonical,
            }),
        ],
        true,
    );
    replay.read_step().expect("read legacy row");
    let step = replay
        .read_step()
        .expect("read conflicting row")
        .expect("row");
    assert!(matches!(step.event, Some(DongleEvent::Malformed { .. })));
    assert_eq!(
        replay.table().view(A).expect("tracker A").last_seen_us,
        1_000_000
    );
    assert_eq!(replay.stats().malformed_reports, 1);
}
