use std::time::{Duration, Instant};

use driver_openut::pose::Pose;
use driver_openut::table::{Action, Callback, Table};
use openut_bridge::{DeviceEntry, MAX_DEVICES, Packet, Payload, Pose as WirePose, TrackingState};

fn list(session: u32, seq: u32, entries: &[(u8, &str)]) -> Packet {
    Packet {
        session,
        seq,
        payload: Payload::DeviceList(
            entries
                .iter()
                .map(|&(device, name)| DeviceEntry {
                    device,
                    name: name.into(),
                })
                .collect(),
        ),
    }
}

fn pose(session: u32, seq: u32, device: u8, x: f32) -> Packet {
    Packet {
        session,
        seq,
        payload: Payload::Pose(WirePose {
            device,
            state: TrackingState::Tracked,
            age_us: 10_000,
            position: [x, 0.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0],
            velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        }),
    }
}

fn packet(table: &mut Table, packet: Packet, at: Instant) -> Vec<Action> {
    let mut actions = Vec::new();
    table.on_packet(packet, at, at, &mut actions);
    actions
}

fn callback(table: &mut Table, callback: Callback, at: Instant) -> Vec<Action> {
    let mut actions = Vec::new();
    table.on_callback(callback, at, &mut actions);
    actions
}

fn finish(table: &mut Table, at: Instant) -> Vec<Action> {
    let mut actions = Vec::new();
    table.finish_frame(at, &mut actions);
    actions
}

fn one_pose(actions: &[Action], expected_slot: usize, expected_index: u32) -> Pose {
    let [Action::Pose { slot, index, pose }] = actions else {
        panic!("expected one pose, got {actions:?}");
    };
    assert_eq!(*slot, expected_slot);
    assert_eq!(*index, expected_index);
    *pose
}

#[test]
fn only_activation_publishes_the_latest_held_pose() {
    let now = Instant::now();
    let mut table = Table::default();
    let entries = [(7, "OpenUT-SyntheticA")];
    assert_eq!(
        packet(&mut table, list(1, 0, &entries), now),
        [Action::Add {
            slot: 0,
            name: entries[0].1.into()
        }]
    );
    assert_eq!(table.on_added(0, true), None);
    assert!(packet(&mut table, list(1, 1, &entries), now).is_empty());
    for seq in 2..5 {
        assert!(
            packet(
                &mut table,
                pose(1, seq, 7, seq as f32),
                now + Duration::from_millis(u64::from(seq))
            )
            .is_empty()
        );
    }
    assert!(finish(&mut table, now + Duration::from_millis(15)).is_empty());
    let activated = callback(
        &mut table,
        Callback::Activated { slot: 0, index: 38 },
        now + Duration::from_millis(20),
    );
    let saved = one_pose(&activated, 0, 38);
    assert!(saved.valid && saved.connected);
    assert_eq!(saved.position, [4.0, 0.0, 0.0]);
    assert!((saved.time_offset + 0.026).abs() < f64::EPSILON);
    let latest = packet(
        &mut table,
        pose(1, 5, 7, 5.0),
        now + Duration::from_millis(21),
    );
    assert_eq!(one_pose(&latest, 0, 38).position, [5.0, 0.0, 0.0]);
}

#[test]
fn timeout_boundaries_are_strict_and_each_transition_is_sent_once() {
    let start = Instant::now();
    let mut table = Table::default();
    let device = [(0, "OpenUT-SyntheticA")];
    packet(&mut table, list(1, 0, &device), start);
    table.on_added(0, true);
    assert!(packet(&mut table, pose(1, 1, 0, 1.0), start).is_empty());
    let at_pose_limit = start + Duration::from_millis(500);
    let at_list_limit = start + Duration::from_secs(3);
    let just_after = Duration::from_millis(1);

    let activate = callback(
        &mut table,
        Callback::Activated { slot: 0, index: 6 },
        at_pose_limit,
    );
    assert!(one_pose(&activate, 0, 6).valid);
    assert!(finish(&mut table, at_pose_limit).is_empty());
    assert_eq!(
        one_pose(&finish(&mut table, at_pose_limit + just_after), 0, 6),
        Pose::invalid(true)
    );
    assert!(finish(&mut table, at_list_limit).is_empty());
    assert_eq!(
        one_pose(&finish(&mut table, at_list_limit + just_after), 0, 6),
        Pose::invalid(false)
    );
    assert!(finish(&mut table, start + Duration::from_secs(4)).is_empty());

    let recovered_at = start + Duration::from_secs(5);
    assert!(packet(&mut table, list(1, 2, &device), recovered_at).is_empty());
    assert_eq!(
        one_pose(&finish(&mut table, recovered_at), 0, 6),
        Pose::invalid(true)
    );
    let recovered = packet(&mut table, pose(1, 3, 0, 5.0), recovered_at);
    assert_eq!(one_pose(&recovered, 0, 6).position, [5.0, 0.0, 0.0]);
}

#[test]
fn stale_or_missing_pose_at_activation_is_invalid() {
    for held in [false, true] {
        let start = Instant::now();
        let mut table = Table::default();
        packet(&mut table, list(1, 0, &[(0, "OpenUT-SyntheticA")]), start);
        table.on_added(0, true);
        if held {
            assert!(packet(&mut table, pose(1, 1, 0, 2.0), start).is_empty());
        }
        let activate = callback(
            &mut table,
            Callback::Activated { slot: 0, index: 9 },
            start + Duration::from_millis(501),
        );
        assert_eq!(one_pose(&activate, 0, 9), Pose::invalid(true));
    }
}

#[test]
fn new_session_requires_new_list_and_reuses_the_name_and_activated_index() {
    let start = Instant::now();
    let mut table = Table::default();
    let name = "OpenUT-SyntheticA";
    packet(&mut table, list(1, 100, &[(5, name)]), start);
    table.on_added(0, true);
    callback(
        &mut table,
        Callback::Activated { slot: 0, index: 42 },
        start,
    );
    assert!(one_pose(&packet(&mut table, pose(1, 101, 5, 1.0), start), 0, 42).valid);

    assert!(packet(&mut table, pose(2, 1, 1, 2.0), start).is_empty());
    assert!(packet(&mut table, pose(1, 102, 5, 3.0), start).is_empty());
    assert!(packet(&mut table, list(1, 103, &[(5, name)]), start).is_empty());
    assert!(packet(&mut table, list(2, 2, &[(1, name)]), start).is_empty());
    assert!(packet(&mut table, pose(2, 3, 5, 4.0), start).is_empty());
    let current = packet(&mut table, pose(2, 4, 1, 5.0), start);
    assert_eq!(one_pose(&current, 0, 42).position, [5.0, 0.0, 0.0]);
    assert_eq!(table.dropped(), 4);
}

#[test]
fn rejects_unknown_device_and_reordered_pose_sequence() {
    let start = Instant::now();
    let mut table = Table::default();
    packet(&mut table, list(1, 0, &[(0, "OpenUT-SyntheticA")]), start);
    table.on_added(0, true);
    callback(
        &mut table,
        Callback::Activated { slot: 0, index: 22 },
        start,
    );
    assert!(packet(&mut table, pose(1, 1000, 1, 1.0), start).is_empty());
    assert!(one_pose(&packet(&mut table, pose(1, 20, 0, 2.0), start), 0, 22).valid);
    assert!(packet(&mut table, pose(1, 19, 0, 3.0), start).is_empty());
    assert_eq!(table.dropped(), 2);
}

#[test]
fn refused_and_deactivated_trackers_do_not_publish_or_get_added_again() {
    let start = Instant::now();
    let mut table = Table::default();
    let devices = [(0, "OpenUT-SyntheticA"), (1, "OpenUT-SyntheticB")];
    packet(&mut table, list(1, 0, &devices), start);
    let refusal = table.on_added(0, false).unwrap();
    assert!(refusal.contains("slot 0"));
    assert!(!refusal.contains(devices[0].1));
    assert_eq!(table.on_added(0, false), None);
    table.on_added(1, true);
    callback(&mut table, Callback::Activated { slot: 1, index: 8 }, start);
    assert!(callback(&mut table, Callback::Deactivated { slot: 1 }, start).is_empty());
    assert!(packet(&mut table, pose(1, 11, 1, 99.0), start).is_empty());
    assert!(packet(&mut table, list(2, 0, &devices), start).is_empty());
    assert!(packet(&mut table, pose(2, 1, 0, 1.0), start).is_empty());
    assert!(packet(&mut table, pose(2, 2, 1, 2.0), start).is_empty());
    assert!(finish(&mut table, start + Duration::from_secs(30)).is_empty());
    assert_eq!(table.dropped(), 3);

    let again = callback(
        &mut table,
        Callback::Activated { slot: 1, index: 12 },
        start,
    );
    assert!(matches!(
        again.as_slice(),
        [
            Action::Log { .. },
            Action::Pose {
                slot: 1,
                index: 12,
                ..
            }
        ]
    ));
    assert!(one_pose(&packet(&mut table, pose(2, 3, 1, 3.0), start), 1, 12).valid);
}

#[test]
fn distinct_names_keep_their_slots_and_cannot_exceed_lifetime_capacity() {
    let start = Instant::now();
    let mut table = Table::default();
    let entries = (0..MAX_DEVICES)
        .map(|device| (device as u8, format!("OpenUT-Synthetic-{device}")))
        .collect::<Vec<_>>();
    let refs = entries
        .iter()
        .map(|(device, name)| (*device, name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        packet(&mut table, list(1, 0, &refs), start).len(),
        MAX_DEVICES
    );
    for slot in 0..MAX_DEVICES {
        table.on_added(slot, false);
    }
    assert!(
        packet(
            &mut table,
            list(2, 0, &[(0, "OpenUT-Synthetic-Extra")]),
            start
        )
        .is_empty()
    );
    assert_eq!(table.dropped(), 1);
}
