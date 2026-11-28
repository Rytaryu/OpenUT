use std::{io::Cursor, num::NonZeroU32};

use openut::run::pipeline::Pipeline;
use openut_bridge::{DeviceEntry, Packet, Payload, Pose, TrackingState};
use openut_dongle::decode::{DongleEvent, PoseSample};
use openut_dongle::record::{RawReport, Record, RecordReader, TrackerLabel};
use openut_dongle::replay::Replay;
use openut_dongle::tracker::Link;
use openut_space::{AxisMap, BodyRotation};

const FIRST_RECORDING: &str = include_str!("fixtures/dongle/m2-1-record.jsonl");
const THREE_TRACKERS: &str = include_str!("fixtures/dongle/m2-3-three-trackers.jsonl");

fn pipeline() -> Pipeline {
    Pipeline::new(
        NonZeroU32::new(17).unwrap(),
        0,
        AxisMap::IDENTITY,
        BodyRotation::IDENTITY,
    )
}

fn entries(slots: &[u8]) -> Vec<DeviceEntry> {
    slots
        .iter()
        .enumerate()
        .map(|(index, slot)| DeviceEntry {
            device: index as u8,
            name: format!("OpenUT-slot{slot}"),
        })
        .collect()
}

fn replay_raw(pipeline: &mut Pipeline, recording: &str) -> Vec<(u64, Packet)> {
    let mut reader = RecordReader::new(Cursor::new(recording)).unwrap();
    let mut output = Vec::new();
    while let Some(record) = reader.read_record().unwrap() {
        let at = record.mono_us();
        let packets = match record {
            Record::Input(report) => pipeline.report(&report, at),
            Record::Omitted {
                tracker: Some(label),
                ..
            } => pipeline.receive_omitted(at, label),
            _ => pipeline.advance_to(at),
        };
        output.extend(packets.into_iter().map(|packet| (at, packet)));
    }
    output
}

#[test]
fn recorded_report_stream_produces_the_expected_lists_poses_and_space() {
    let mut pipeline = Pipeline::new(
        NonZeroU32::new(17).unwrap(),
        0,
        "-z+x+y".parse().unwrap(),
        "-z+y+x".parse().unwrap(),
    );
    let output = replay_raw(&mut pipeline, THREE_TRACKERS);
    let lists: Vec<_> = output
        .iter()
        .filter_map(|(at, packet)| match &packet.payload {
            Payload::DeviceList(list) => Some((*at, list.as_slice())),
            _ => None,
        })
        .collect();
    assert_eq!(
        lists,
        [
            (200_000, entries(&[1]).as_slice()),
            (300_000, entries(&[1, 2]).as_slice()),
            (500_000, entries(&[1, 2, 3]).as_slice()),
            (1_000_000, entries(&[1, 2, 3]).as_slice()),
        ]
    );

    let poses: Vec<_> = output
        .iter()
        .filter_map(|(at, packet)| match &packet.payload {
            Payload::Pose(pose) => {
                assert_eq!(pose.position, [-3.0, 1.0, 2.0]);
                assert!((pose.rotation[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
                assert!((pose.rotation[2] + std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
                assert_eq!(pose.rotation[1], 0.0);
                assert_eq!(pose.rotation[3], 0.0);
                assert_eq!(pose.velocity, [0.0; 3]);
                assert_eq!(pose.angular_velocity, [0.0; 3]);
                assert_eq!(pose.age_us, 0);
                Some((*at, pose.device, pose.state))
            }
            _ => None,
        })
        .collect();
    use TrackingState::{RotationOnly, Tracked};
    assert_eq!(
        poses,
        [
            (200_000, 0, Tracked),
            (300_000, 1, Tracked),
            (500_000, 2, Tracked),
            (800_000, 0, RotationOnly),
            (1_000_000, 1, Tracked),
            (1_100_000, 0, Tracked),
        ]
    );
    assert!(pipeline.advance_to(1_999_999).is_empty());
    let repeated = pipeline.advance_to(2_000_000);
    assert_eq!(
        repeated[0].payload,
        Payload::DeviceList(entries(&[1, 2, 3]))
    );
    assert!(pipeline.advance_to(2_000_000).is_empty());
    for (seq, packet) in output
        .iter()
        .map(|(_, packet)| packet)
        .chain(&repeated)
        .enumerate()
    {
        assert_eq!(packet.seq, seq as u32);
        assert_eq!(packet.session, 17);
        let bytes = openut_bridge::encode(packet).unwrap();
        assert_eq!(openut_bridge::decode(&bytes).unwrap(), *packet);
    }
    assert_eq!(pipeline.stats().poses, 6);
    assert_eq!(pipeline.stats().device_lists, 5);
}

#[test]
fn redacted_replay_advances_time_without_inventing_a_tracker_slot() {
    let mut pipeline = pipeline();
    let mut replay = Replay::new(Cursor::new(FIRST_RECORDING)).unwrap();
    let mut output = Vec::new();
    while let Some(step) = replay.read_step().unwrap() {
        let packets = match step.event {
            Some(event) => pipeline.apply_event(step.mono_us, step.mono_us, &event),
            None => match step.omitted_tracker {
                Some(label) => pipeline.receive_omitted(step.mono_us, label),
                None => pipeline.advance_to(step.mono_us),
            },
        };
        output.extend(
            packets
                .into_iter()
                .map(|packet| (step.mono_us, packet.payload)),
        );
    }
    assert_eq!(output, [(1_000_412, Payload::DeviceList(Vec::new()))]);
    assert_eq!(pipeline.stats().poses, 0);
    assert_eq!(pipeline.stats().malformed_reports, 1);
    let status = pipeline.status();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].label, TrackerLabel('A'));
    assert_eq!(status[0].slot, None);
    assert_eq!(status[0].link, Link::Receiving);
}

fn pose(tracker: char, slot: u8, state: i8, position: [f32; 3], rotation: [f32; 4]) -> DongleEvent {
    DongleEvent::Pose {
        tracker: TrackerLabel(tracker),
        sample: PoseSample {
            slot,
            tracking_status: state,
            position_m: position,
            rotation_wxyz: rotation,
            buttons_raw: 0,
            acceleration: [5.0; 3],
            angular_velocity: [6.0; 4],
            extra: Vec::new(),
        },
    }
}

fn last_pose(packets: &[Packet]) -> &Pose {
    let Payload::Pose(pose) = &packets.last().expect("pose output").payload else {
        panic!("last packet should be the pose");
    };
    pose
}

#[test]
fn pose_hold_uses_only_that_tracker_and_a_valid_slot_and_clears_on_unpair() {
    let mut pipeline = pipeline();
    let unit = [1.0, 0.0, 0.0, 0.0];
    let rotated = [0.0, 1.0, 0.0, 0.0];
    assert!(
        pipeline
            .apply_event(1, 1, &pose('A', 255, 2, [1.0; 3], unit))
            .is_empty()
    );
    assert_eq!(pipeline.stats().unknown_slot_poses, 1);

    let first = pipeline.apply_event(10, 10, &pose('A', 3, 3, [1.0; 3], rotated));
    assert_eq!(first[0].payload, Payload::DeviceList(entries(&[3])));
    assert_eq!(last_pose(&first).state, TrackingState::Invalid);
    let tracked = pipeline.apply_event(20, 25, &pose('A', 3, 2, [2.0; 3], unit));
    assert_eq!(last_pose(&tracked).state, TrackingState::Tracked);
    assert_eq!(last_pose(&tracked).age_us, 5);
    let second = pipeline.apply_event(21, 20, &pose('B', 1, 2, [9.0; 3], unit));
    assert_eq!(second[0].payload, Payload::DeviceList(entries(&[3, 1])));
    assert_eq!(last_pose(&second).age_us, 0);
    let loss = pipeline.apply_event(30, u64::MAX, &pose('A', 3, 4, [f32::NAN; 3], rotated));
    assert_eq!(last_pose(&loss).device, 0);
    assert_eq!(last_pose(&loss).state, TrackingState::RotationOnly);
    assert_eq!(last_pose(&loss).position, [2.0; 3]);
    assert_eq!(last_pose(&loss).rotation, rotated);
    assert_eq!(last_pose(&loss).age_us, u32::MAX);
    let corrupt = pipeline.apply_event(40, 40, &pose('A', 3, 2, [f32::NAN; 3], [0.0; 4]));
    assert_eq!(last_pose(&corrupt).state, TrackingState::Invalid);
    assert!(openut_bridge::encode(&corrupt[0]).is_ok());
    assert_eq!(pipeline.stats().invalid_pose_samples, 1);
    assert!(
        pipeline
            .apply_event(45, 45, &pose('C', 3, 2, [8.0; 3], unit))
            .is_empty()
    );
    assert_eq!(pipeline.stats().conflicting_slot_poses, 1);
    assert!(
        pipeline
            .apply_event(
                50,
                50,
                &DongleEvent::Pair {
                    tracker: TrackerLabel('A'),
                    unpaired: true,
                }
            )
            .is_empty()
    );
    let reassigned = pipeline.apply_event(51, 51, &pose('C', 3, 4, [8.0; 3], rotated));
    assert_eq!(reassigned.len(), 1);
    assert_eq!(last_pose(&reassigned).device, 0);
    assert_eq!(last_pose(&reassigned).state, TrackingState::Invalid);
}

#[test]
fn console_status_displays_only_the_safe_tracker_label_and_fields() {
    let mut pipeline = pipeline();
    for (at, text) in ["NADSMOCK-SERIAL-86301", "WpMOCK-WIFI-SECRET"]
        .into_iter()
        .enumerate()
    {
        let mut bytes = vec![
            0x28,
            0,
            0,
            0xaa,
            1,
            0xaa,
            0xbb,
            0xcc,
            0xdd,
            1,
            1,
            text.len() as u8,
        ];
        bytes.extend_from_slice(text.as_bytes());
        let at = at as u64;
        assert!(
            pipeline
                .report(
                    &RawReport {
                        mono_us: at,
                        wall_ms: 0,
                        bytes
                    },
                    at
                )
                .is_empty()
        );
    }
    let status = pipeline.status()[0].to_string();
    assert!(status.contains("トラッカー A"));
    assert!(status.contains("受信中"));
    for private in ["MOCK-SERIAL-86301", "MOCK-WIFI-SECRET", "01aabbccdd"] {
        assert!(!status.contains(private));
    }
    assert!(!status.contains("枠"));
}
