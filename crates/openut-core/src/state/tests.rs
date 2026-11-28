use super::{
    Connection, DeviceView, DongleState, HostBy, HostWifi, HubState, Link, Phase, Role, Snapshot,
    State, StateTally, StateUpdate, TrackerView,
};
use crate::{
    event::{Battery, Event, OetKind, PoseStatus, Timed},
    time::{LogTime, UtcOffset},
    tracker::{HubNumber, Serial, TrackerKey},
};

fn apply(state: &mut State, at: i64, event: Event) -> StateUpdate {
    state.apply(&Timed {
        at: LogTime(at),
        offset: UtcOffset(0),
        event,
    })
}

fn tracker(snapshot: &Snapshot, slot: u16) -> &TrackerView {
    snapshot
        .trackers
        .iter()
        .find(|tracker| tracker.key == TrackerKey(slot))
        .expect("the fixture has a tracker at this slot")
}

fn report(slot: u16, connected: bool, host: bool, map_ready: bool, usb: Option<bool>) -> Event {
    Event::Status {
        tracker: TrackerKey(slot),
        connected,
        host,
        map_ready,
        usb,
        battery: None,
    }
}

fn add(slot: u16) -> Event {
    Event::TrackerAdded {
        tracker: TrackerKey(slot),
        usb: false,
    }
}

fn remove(slot: u16) -> Event {
    Event::TrackerRemoved {
        tracker: TrackerKey(slot),
        usb: false,
    }
}

fn link(slot: u16, connected: bool) -> Event {
    Event::ClientLink {
        tracker: TrackerKey(slot),
        connected,
    }
}

fn pose(slot: u16, status: PoseStatus) -> Event {
    Event::Pose {
        tracker: TrackerKey(slot),
        status,
    }
}

fn oet(slot: u16, kind: OetKind) -> Event {
    Event::Oet {
        tracker: TrackerKey(slot),
        kind,
    }
}

fn serial(slot: u16, value: &str) -> Event {
    device_info(slot, value, None, None)
}

fn device_info(slot: u16, value: &str, number: Option<u16>, firmware: Option<&str>) -> Event {
    Event::SerialKnown {
        tracker: TrackerKey(slot),
        serial: Serial(value.into()),
        hub_number: number.map(HubNumber),
        firmware: firmware.map(str::to_owned),
    }
}

fn assign_host(slot: u16) -> Event {
    Event::HostAssigned {
        tracker: TrackerKey(slot),
    }
}

fn unanswered() -> Event {
    Event::WifiRequestUnanswered { tracker: None }
}

#[test]
fn client_events_do_not_replace_host_evidence_for_the_same_connection() {
    let mut state = State::default();
    apply(&mut state, 10, report(2, true, true, false, None));
    let reported_host = state.snapshot(LogTime(10)).host;
    apply(
        &mut state,
        20,
        Event::WifiAssigned {
            tracker: TrackerKey(2),
        },
    );
    apply(&mut state, 30, link(2, true));
    let snapshot = state.snapshot(LogTime(30));
    assert_eq!(snapshot.host, reported_host);
    assert_eq!(tracker(&snapshot, 2).role, Role::Host);
}

fn with_phase(phase: Phase) -> (State, LogTime) {
    let mut state = State::default();
    let slot = 2;
    apply(&mut state, 10, add(slot));
    let transition = match phase {
        Phase::Connected => return (state, LogTime(10)),
        Phase::Disconnected => remove(slot),
        Phase::WaitingWifi => unanswered(),
        Phase::MapSyncing => link(slot, true),
        Phase::Relocalizing => report(slot, true, false, true, None),
        Phase::Tracking => pose(slot, PoseStatus::Ok),
    };
    assert!(!apply(&mut state, 20, transition).out_of_order);
    assert_eq!(tracker(&state.snapshot(LogTime(20)), slot).phase, phase);
    (state, LogTime(20))
}

fn assert_idle(update: &StateUpdate) {
    assert!(update.connections.is_empty());
    assert!(!update.out_of_order);
    assert!(update.tally.is_none());
}

#[test]
fn host_assignment_starts_sync_or_preserves_an_active_phase() {
    for phase in [
        Phase::Connected,
        Phase::WaitingWifi,
        Phase::MapSyncing,
        Phase::Relocalizing,
        Phase::Tracking,
    ] {
        let (mut state, since) = with_phase(phase);
        apply(&mut state, 30, add(4));
        assert_idle(&apply(&mut state, 40, assign_host(2)));
        let snapshot = state.snapshot(LogTime(40));
        let host = tracker(&snapshot, 2);
        assert_eq!(host.role, Role::Host);
        let expected = match phase {
            Phase::Connected | Phase::WaitingWifi => (Phase::MapSyncing, LogTime(40)),
            _ => (phase, since),
        };
        assert_eq!((host.phase, host.since), expected);
        let peer = tracker(&snapshot, 4);
        assert_eq!(
            (peer.phase, peer.since, peer.role),
            (Phase::WaitingWifi, LogTime(40), Role::Unknown)
        );
    }
}

#[test]
fn wifi_assignment_sets_client_role_without_restarting_wait() {
    for phase in [Phase::Connected, Phase::WaitingWifi] {
        let (mut state, since) = with_phase(phase);
        apply(&mut state, 30, Event::HostWifiObtained { host: None });
        let event = Event::WifiAssigned {
            tracker: TrackerKey(2),
        };
        assert_idle(&apply(&mut state, 40, event.clone()));
        assert_idle(&apply(&mut state, 50, event));

        let snapshot = state.snapshot(LogTime(50));
        let view = tracker(&snapshot, 2);
        assert_eq!((view.phase, view.role), (Phase::WaitingWifi, Role::Client));
        assert_eq!(
            view.since,
            if phase == Phase::Connected {
                LogTime(40)
            } else {
                since
            }
        );
    }
}

#[test]
fn client_link_syncs_then_returns_to_waiting_when_it_drops() {
    for phase in [Phase::Connected, Phase::WaitingWifi] {
        let (mut state, _) = with_phase(phase);
        assert_idle(&apply(&mut state, 30, link(2, true)));
        let snapshot = state.snapshot(LogTime(30));
        let view = tracker(&snapshot, 2);
        assert_eq!(
            (view.phase, view.role, view.since),
            (Phase::MapSyncing, Role::Client, LogTime(30))
        );
        assert_idle(&apply(&mut state, 40, link(2, false)));
        let snapshot = state.snapshot(LogTime(40));
        let view = tracker(&snapshot, 2);
        assert_eq!(
            (view.phase, view.role, view.since),
            (Phase::WaitingWifi, Role::Client, LogTime(40))
        );
    }
}

#[test]
fn client_link_loss_is_counted_without_rolling_back_positioning() {
    for phase in [Phase::Relocalizing, Phase::Tracking] {
        let (mut state, since) = with_phase(phase);
        let update = apply(&mut state, 30, link(2, false));
        assert!(!update.out_of_order);
        assert!(matches!(update.tally, Some(StateTally::ClientLinkLost)));
        assert!(update.connections.is_empty());
        let snapshot = state.snapshot(LogTime(30));
        let view = tracker(&snapshot, 2);
        assert_eq!((view.phase, view.since), (phase, since));
    }
}

#[test]
fn map_ready_reports_advance_pre_tracking_phases_and_leave_tracking_alone() {
    for phase in [
        Phase::Connected,
        Phase::WaitingWifi,
        Phase::MapSyncing,
        Phase::Tracking,
    ] {
        let (mut state, since) = with_phase(phase);
        assert_idle(&apply(&mut state, 30, report(2, true, false, true, None)));
        assert_idle(&apply(&mut state, 40, report(2, true, false, true, None)));
        assert_eq!(state.out_of_order(), 0);
        let snapshot = state.snapshot(LogTime(40));
        let view = tracker(&snapshot, 2);
        assert_eq!(
            (view.phase, view.since),
            if phase == Phase::Tracking {
                (phase, since)
            } else {
                (Phase::Relocalizing, LogTime(30))
            }
        );

        let update = apply(&mut state, 50, report(2, false, false, false, None));
        assert!(update.out_of_order);
        assert!(update.connections.is_empty());
        assert_eq!(state.out_of_order(), 1);
        let snapshot = state.snapshot(LogTime(50));
        let after = tracker(&snapshot, 2);
        assert_eq!(
            (after.phase, after.since, after.conn),
            (view.phase, view.since, view.conn)
        );
    }
}

#[test]
fn status_updates_phase_connection_path_and_host_evidence_independently() {
    for (usb, connection) in [(true, Link::Usb), (false, Link::Dongle)] {
        let mut state = State::default();
        apply(&mut state, 10, report(2, true, false, false, None));
        let snapshot = state.snapshot(LogTime(10));
        let view = tracker(&snapshot, 2);
        assert_eq!(view.link, Link::Unknown);
        assert!(view.inferred);

        assert_idle(&apply(
            &mut state,
            20,
            report(2, true, true, true, Some(usb)),
        ));
        let snapshot = state.snapshot(LogTime(20));
        let host = snapshot
            .host
            .as_ref()
            .expect("connected host report was applied");
        let updated = tracker(&snapshot, 2);
        assert_eq!(host.key, updated.key);
        assert_eq!(host.conn, view.conn);
        assert_eq!(host.by, HostBy::Report);
        assert!(matches!(snapshot.host_wifi, HostWifi::Unknown));
        assert_eq!(
            (updated.phase, updated.role, updated.link),
            (Phase::Relocalizing, Role::Host, connection)
        );

        assert_idle(&apply(
            &mut state,
            30,
            report(2, true, false, false, Some(!usb)),
        ));
        let snapshot = state.snapshot(LogTime(30));
        let view = tracker(&snapshot, 2);
        assert_eq!(
            (view.phase, view.role, view.link, view.since),
            (Phase::Relocalizing, Role::Host, connection, LogTime(20))
        );
    }
}

#[test]
fn disconnected_report_fills_usb_path_without_ending_the_connection() {
    let mut state = State::default();
    apply(&mut state, 10, pose(2, PoseStatus::Ok));
    let before = state.snapshot(LogTime(10));
    let inferred = tracker(&before, 2);
    assert_eq!(
        (inferred.phase, inferred.link),
        (Phase::Tracking, Link::Unknown)
    );

    let update = apply(&mut state, 20, report(2, false, false, true, Some(true)));
    assert_eq!(
        update,
        StateUpdate {
            out_of_order: true,
            ..StateUpdate::default()
        }
    );
    assert_eq!(state.out_of_order(), 1);
    let expected = TrackerView {
        link: Link::Usb,
        ..inferred.clone()
    };
    assert_eq!(tracker(&state.snapshot(LogTime(20)), 2), &expected);

    assert_idle(&apply(&mut state, 30, Event::DongleRemoved));
    assert_eq!(tracker(&state.snapshot(LogTime(30)), 2), &expected);
    assert_eq!(state.out_of_order(), 1);
}

#[test]
fn pose_ok_and_oet_relocation_both_set_tracking_once() {
    for first in [pose(2, PoseStatus::Ok), oet(2, OetKind::RelocBack)] {
        let (mut state, _) = with_phase(Phase::Relocalizing);
        assert_idle(&apply(&mut state, 30, first));
        assert_idle(&apply(&mut state, 40, pose(2, PoseStatus::Ok)));
        assert_idle(&apply(&mut state, 50, oet(2, OetKind::RelocBack)));
        let snapshot = state.snapshot(LogTime(50));
        let view = tracker(&snapshot, 2);
        assert_eq!((view.phase, view.since), (Phase::Tracking, LogTime(30)));
        assert!(view.tracked_since_connect);
    }
}

#[test]
fn loss_moves_tracking_or_map_sync_into_relocalization() {
    for phase in [Phase::Tracking, Phase::MapSyncing] {
        for loss in [
            pose(2, PoseStatus::Lost),
            pose(2, PoseStatus::RecentlyLost),
            oet(2, OetKind::LostTracking),
        ] {
            let (mut state, _) = with_phase(phase);
            assert_idle(&apply(&mut state, 30, loss));
            let snapshot = state.snapshot(LogTime(30));
            let view = tracker(&snapshot, 2);
            assert_eq!((view.phase, view.since), (Phase::Relocalizing, LogTime(30)));
            assert_eq!(view.tracked_since_connect, phase == Phase::Tracking);
        }
    }
}

#[test]
fn early_pose_reports_are_ignored_but_later_diagnostics_are_counted() {
    for phase in [Phase::Connected, Phase::WaitingWifi] {
        let (mut state, since) = with_phase(phase);
        for status in [
            PoseStatus::Lost,
            PoseStatus::RecentlyLost,
            PoseStatus::Other(42),
        ] {
            assert_idle(&apply(&mut state, 30, pose(2, status)));
        }
        let snapshot = state.snapshot(LogTime(30));
        let view = tracker(&snapshot, 2);
        assert_eq!((view.phase, view.since), (phase, since));
        assert!(!view.tracked_since_connect);
    }

    let (mut state, since) = with_phase(Phase::Tracking);
    let other = apply(&mut state, 30, pose(2, PoseStatus::Other(42)));
    assert!(matches!(other.tally, Some(StateTally::OtherPose)));
    assert!(!other.out_of_order);
    let overspeed = apply(&mut state, 40, oet(2, OetKind::Overspeed));
    assert!(matches!(overspeed.tally, Some(StateTally::Overspeed)));
    assert!(!overspeed.out_of_order);
    let snapshot = state.snapshot(LogTime(40));
    let view = tracker(&snapshot, 2);
    assert_eq!((view.phase, view.since), (Phase::Tracking, since));
}

#[test]
fn unseen_tracker_phases_are_inferred_from_the_first_position_or_link_report() {
    for (event, phase, connection) in [
        (pose(2, PoseStatus::Ok), Phase::Tracking, Link::Unknown),
        (oet(2, OetKind::RelocBack), Phase::Tracking, Link::Unknown),
        (
            pose(2, PoseStatus::Lost),
            Phase::Relocalizing,
            Link::Unknown,
        ),
        (
            pose(2, PoseStatus::RecentlyLost),
            Phase::Relocalizing,
            Link::Unknown,
        ),
        (
            oet(2, OetKind::LostTracking),
            Phase::Relocalizing,
            Link::Unknown,
        ),
        (link(2, true), Phase::MapSyncing, Link::Unknown),
        (
            report(2, true, false, true, Some(true)),
            Phase::Relocalizing,
            Link::Usb,
        ),
        (
            report(2, true, false, false, Some(false)),
            Phase::Connected,
            Link::Dongle,
        ),
    ] {
        let mut state = State::default();
        let update = apply(&mut state, 10, event);
        assert!(!update.out_of_order);
        assert_eq!(update.connections.len(), 1);
        let snapshot = state.snapshot(LogTime(10));
        let view = tracker(&snapshot, 2);
        assert_eq!(
            (view.phase, view.link, view.since),
            (phase, connection, LogTime(10))
        );
        assert!(view.inferred);
        assert_eq!(view.tracked_since_connect, phase == Phase::Tracking);
    }
}

#[test]
fn unseen_disconnected_reports_and_serials_do_not_start_connections() {
    let mut state = State::default();
    assert_idle(&apply(
        &mut state,
        10,
        report(2, false, true, true, Some(true)),
    ));
    let update = apply(&mut state, 20, serial(4, "SYNTHETIC-TRACKER-A"));
    assert!(update.connections.is_empty());
    let snapshot = state.snapshot(LogTime(20));
    assert!(snapshot.trackers.is_empty());
    assert!(snapshot.host.is_none());
}

#[test]
fn slot_seen_disconnect_cannot_be_resurrected_by_a_pose() {
    let (mut state, since) = with_phase(Phase::Disconnected);
    let update = apply(&mut state, 30, pose(2, PoseStatus::Ok));
    assert!(update.out_of_order);
    assert!(update.connections.is_empty());
    let snapshot = state.snapshot(LogTime(30));
    let view = tracker(&snapshot, 2);
    assert_eq!((view.phase, view.since), (Phase::Disconnected, since));
}

#[test]
fn unanswered_without_slot_requires_exactly_one_non_host_candidate() {
    let mut state = State::default();
    let update = apply(&mut state, 10, report(2, true, true, false, None));
    assert_eq!(update.connections.len(), 1);
    assert!(!update.out_of_order);
    assert_idle(&apply(&mut state, 20, unanswered()));
    let snapshot = state.snapshot(LogTime(20));
    assert_eq!(
        (tracker(&snapshot, 2).phase, tracker(&snapshot, 2).role),
        (Phase::Connected, Role::Host)
    );

    apply(&mut state, 30, report(4, true, false, false, None));
    apply(&mut state, 40, report(0, true, false, false, None));
    assert_idle(&apply(&mut state, 50, unanswered()));
    let snapshot = state.snapshot(LogTime(50));
    for slot in [0, 4] {
        let view = tracker(&snapshot, slot);
        assert_eq!((view.phase, view.role), (Phase::Connected, Role::Unknown));
    }
    assert!(matches!(snapshot.host_wifi, HostWifi::Unknown));

    assert_idle(&apply(&mut state, 60, link(0, true)));
    assert_idle(&apply(&mut state, 70, unanswered()));
    assert_idle(&apply(&mut state, 80, unanswered()));
    let snapshot = state.snapshot(LogTime(80));
    let client = tracker(&snapshot, 4);
    assert_eq!(
        (client.phase, client.role, client.since),
        (Phase::WaitingWifi, Role::Client, LogTime(70))
    );
    assert_eq!(tracker(&snapshot, 0).phase, Phase::MapSyncing);
    assert_eq!(
        snapshot.host.expect("host remains unchanged").key,
        TrackerKey(2)
    );
}

#[test]
fn serial_movement_and_all_disconnect_kinds_clear_the_slot_identity() {
    let synthetic = "SYNTHETIC-TRACKER-A";
    for disconnect in [
        remove(4),
        Event::DongleRemoved,
        Event::ServerStarted,
        Event::ServerStopped,
    ] {
        let mut state = State::default();
        apply(&mut state, 10, add(2));
        apply(&mut state, 20, add(4));
        assert_idle(&apply(&mut state, 30, serial(2, synthetic)));
        assert_idle(&apply(&mut state, 40, serial(4, synthetic)));
        let snapshot = state.snapshot(LogTime(40));
        let from = tracker(&snapshot, 2);
        let to = tracker(&snapshot, 4);
        assert!(from.serial.is_none());
        assert_eq!(
            to.serial.as_ref().map(|serial| serial.0.as_str()),
            Some(synthetic)
        );
        assert_eq!((to.phase, to.since), (Phase::Connected, LogTime(20)));

        apply(&mut state, 50, disconnect);
        let snapshot = state.snapshot(LogTime(50));
        let view = tracker(&snapshot, 4);
        assert!(view.serial.is_none());
        assert_eq!(
            (view.phase, view.link, view.role),
            (Phase::Disconnected, Link::Unknown, Role::Unknown)
        );
    }
}

#[test]
fn power_off_notices_and_pc_commands_leave_tracker_state_intact() {
    let (mut state, since) = with_phase(Phase::Tracking);
    assert_idle(&apply(
        &mut state,
        30,
        Event::PowerOffNotice {
            tracker: TrackerKey(2),
        },
    ));
    assert_idle(&apply(
        &mut state,
        40,
        Event::PowerOffNotice {
            tracker: TrackerKey(4),
        },
    ));
    assert_idle(&apply(
        &mut state,
        50,
        Event::PcCommand {
            tracker: None,
            kind: "stop".into(),
        },
    ));
    let snapshot = state.snapshot(LogTime(50));
    assert_eq!(snapshot.trackers.len(), 1);
    let view = tracker(&snapshot, 2);
    assert_eq!((view.phase, view.since), (Phase::Tracking, since));
}

#[test]
fn the_same_timed_lifecycle_works_for_one_and_five_tracker_slots() {
    let slots = [2, 8, 15, 80, 500];
    for count in [1, slots.len()] {
        let mut state = State::default();
        for (index, &slot) in slots[..count].iter().enumerate() {
            let base = index as i64 * 100;
            let sequence = [
                (10, add(slot), Phase::Connected),
                (20, link(slot, true), Phase::MapSyncing),
                (
                    30,
                    report(slot, true, false, true, None),
                    Phase::Relocalizing,
                ),
                (40, pose(slot, PoseStatus::Ok), Phase::Tracking),
                (50, remove(slot), Phase::Disconnected),
            ];
            for (time, event, phase) in sequence {
                let at = base + time;
                let update = apply(&mut state, at, event);
                assert!(!update.out_of_order);
                let snapshot = state.snapshot(LogTime(at));
                let view = tracker(&snapshot, slot);
                assert_eq!((view.phase, view.since), (phase, LogTime(at)));
                assert!(!view.inferred);
            }
        }
        assert_eq!(state.snapshot(LogTime(1_000)).trackers.len(), count);
    }
}

const BATTERY: Battery = Battery {
    percent: 73,
    charging: Some(true),
};

fn battery_report(slot: u16, usb: Option<bool>) -> Event {
    Event::Status {
        tracker: TrackerKey(slot),
        connected: true,
        host: false,
        map_ready: false,
        usb,
        battery: Some(BATTERY),
    }
}

fn device<'a>(snapshot: &'a Snapshot, serial: &str) -> &'a DeviceView {
    snapshot
        .devices
        .iter()
        .find(|device| {
            device
                .serial
                .as_ref()
                .is_some_and(|value| value.0 == serial)
        })
        .expect("the fixture includes this device")
}

#[test]
fn battery_and_hub_number_belong_to_the_current_connection() {
    for end in [
        remove(2),
        Event::DongleRemoved,
        Event::ServerStarted,
        Event::ServerStopped,
        add(2),
    ] {
        let mut state = State::default();
        apply(&mut state, 10, add(2));
        apply(&mut state, 20, battery_report(2, None));
        apply(
            &mut state,
            30,
            device_info(2, "TRACKER-SYNTHETIC-A", Some(1), None),
        );
        apply(&mut state, 40, report(2, true, false, false, None));
        apply(&mut state, 50, serial(2, "TRACKER-SYNTHETIC-A"));
        let initial = state.snapshot(LogTime(50));
        assert_eq!(tracker(&initial, 2).battery, Some(BATTERY));
        assert_eq!(tracker(&initial, 2).hub_number, Some(HubNumber(1)));

        apply(
            &mut state,
            60,
            device_info(2, "TRACKER-SYNTHETIC-A", Some(3), None),
        );
        assert_eq!(
            tracker(&state.snapshot(LogTime(60)), 2).hub_number,
            Some(HubNumber(3))
        );
        apply(&mut state, 2_000, end);
        let ended = state.snapshot(LogTime(2_000));
        let view = tracker(&ended, 2);
        assert_eq!((view.battery, view.hub_number), (None, None));

        apply(&mut state, 4_000, add(2));
        let started = state.snapshot(LogTime(4_000));
        let view = tracker(&started, 2);
        assert_eq!((view.battery, view.hub_number), (None, None));
        assert_ne!(view.conn, tracker(&initial, 2).conn);
    }
}

#[test]
fn inferred_status_keeps_battery_and_a_dongle_removal_leaves_usb_metadata() {
    let mut state = State::default();
    apply(&mut state, 10, battery_report(2, Some(true)));
    apply(
        &mut state,
        20,
        device_info(2, "TRACKER-SYNTHETIC-USB", Some(1), None),
    );
    apply(&mut state, 30, battery_report(4, None));
    apply(
        &mut state,
        40,
        device_info(4, "TRACKER-SYNTHETIC-WIRELESS", Some(2), None),
    );
    let before = state.snapshot(LogTime(40));
    for slot in [2, 4] {
        assert!(tracker(&before, slot).inferred);
        assert_eq!(tracker(&before, slot).battery, Some(BATTERY));
    }

    apply(&mut state, 50, Event::DongleRemoved);
    let after = state.snapshot(LogTime(50));
    assert_eq!(tracker(&after, 2), tracker(&before, 2));
    assert_eq!(tracker(&after, 4).battery, None);
    assert_eq!(tracker(&after, 4).hub_number, None);
}

#[test]
fn roster_preserves_firmware_without_reusing_the_hub_number_on_another_slot() {
    let value = "TRACKER-SYNTHETIC-A";
    let mut state = State::default();
    assert!(state.snapshot(LogTime(0)).roster.is_empty());
    apply(&mut state, 10, add(2));
    apply(&mut state, 20, add(4));
    apply(&mut state, 30, device_info(2, value, Some(1), Some("1.2")));
    apply(&mut state, 40, serial(4, value));
    let moved = state.snapshot(LogTime(40));
    assert_eq!(moved.roster.len(), 1);
    assert_eq!(moved.roster[0].serial, Serial(value.into()));
    assert_eq!(moved.roster[0].firmware.as_deref(), Some("1.2"));
    assert_eq!(moved.roster[0].last_seen, LogTime(40));
    let old_slot = tracker(&moved, 2);
    assert_eq!(
        (old_slot.serial.as_ref(), old_slot.hub_number),
        (None, None)
    );
    assert_eq!(tracker(&moved, 4).hub_number, None);

    apply(&mut state, 50, device_info(4, value, Some(2), Some("1.3")));
    apply(&mut state, 60, report(4, true, false, false, None));
    let updated = state.snapshot(LogTime(60));
    assert_eq!(updated.roster[0].firmware.as_deref(), Some("1.3"));
    assert_eq!(updated.roster[0].last_seen, LogTime(60));
    assert_eq!(device(&updated, value).hub_number, Some(HubNumber(2)));

    for (at, event) in [
        (70, remove(4)),
        (80, Event::ServerStarted),
        (90, Event::ServerStopped),
    ] {
        apply(&mut state, at, event);
        let snapshot = state.snapshot(LogTime(at));
        assert_eq!(snapshot.roster, updated.roster);
        assert_eq!(device(&snapshot, value).hub_number, None);
    }
}

#[test]
fn roster_capacity_evicts_the_oldest_disconnected_identity_first() {
    for all_active in [false, true] {
        let mut state = State::default();
        for i in 0..=16_u16 {
            let slot = if all_active || i == 0 { i } else { 1 };
            let at = i64::from(i) * 10 + 1;
            apply(&mut state, at, add(slot));
            apply(
                &mut state,
                at + 1,
                serial(slot, &format!("TRACKER-SYNTHETIC-{i:02}")),
            );
            if !all_active && i > 0 {
                apply(&mut state, at + 2, remove(slot));
            }
        }
        let snapshot = state.snapshot(LogTime(200));
        assert_eq!(snapshot.roster.len(), 16);
        let oldest_to_evict = if all_active { "00" } else { "01" };
        assert!(
            !snapshot
                .roster
                .iter()
                .any(|identity| identity.serial.0.ends_with(oldest_to_evict))
        );
        if !all_active {
            assert_eq!(
                device(&snapshot, "TRACKER-SYNTHETIC-00").connection,
                Connection::Dongle
            );
        }
    }
}

#[test]
fn three_and_five_devices_use_the_current_slot_and_host_connection() {
    let slots = [0, 2, 4, 1, 3];
    for count in [3, 5] {
        let mut state = State::default();
        for (i, &slot) in slots[..count].iter().enumerate() {
            let at = i as i64 * 10;
            apply(
                &mut state,
                at + 1,
                battery_report(slot, (slot == 0).then_some(true)),
            );
            apply(
                &mut state,
                at + 2,
                device_info(
                    slot,
                    &format!("TRACKER-SYNTHETIC-{slot}"),
                    Some(slot + 1),
                    Some("1.2"),
                ),
            );
            apply(&mut state, at + 3, pose(slot, PoseStatus::Ok));
        }
        apply(&mut state, 60, assign_host(2));
        let first = state.snapshot(LogTime(60));
        assert_eq!(first.devices.len(), count);
        for &slot in &slots[..count] {
            let value = format!("TRACKER-SYNTHETIC-{slot}");
            let view = device(&first, &value);
            let source = tracker(&first, slot);
            assert_eq!(view.key, Some(TrackerKey(slot)));
            assert_eq!(view.conn, Some(source.conn));
            assert_eq!(view.hub_number, Some(HubNumber(slot + 1)));
            assert_eq!(view.battery, Some(BATTERY));
            assert_eq!(view.phase, Phase::Tracking);
            assert_eq!(view.host, slot == 2);
            let connection = if slot == 0 {
                Connection::Usb
            } else {
                Connection::Dongle
            };
            assert_eq!(view.connection, connection);
        }

        apply(&mut state, 70, assign_host(4));
        let after = state.snapshot(LogTime(70));
        let mut expected = first.devices;
        for entry in &mut expected {
            entry.host = entry.key == Some(TrackerKey(4));
        }
        assert_eq!(after.devices, expected);
    }
}

#[test]
fn provisional_device_becomes_known_without_inheriting_a_previous_hosts_connection() {
    let mut state = State::default();
    apply(&mut state, 10, add(3));
    let first = state.snapshot(LogTime(10));
    let [provisional] = first.devices.as_slice() else {
        panic!("the connected slot has one provisional device");
    };
    let old_conn = Some(tracker(&first, 3).conn);
    assert_eq!(
        (provisional.key, provisional.conn),
        (Some(TrackerKey(3)), old_conn)
    );
    assert_eq!(
        (provisional.serial.as_ref(), provisional.hub_number),
        (None, None)
    );
    assert!(provisional.firmware.is_none());

    apply(
        &mut state,
        20,
        device_info(3, "TRACKER-SYNTHETIC-A", Some(1), Some("1.2")),
    );
    apply(&mut state, 30, battery_report(3, None));
    apply(&mut state, 40, assign_host(3));
    let known = state.snapshot(LogTime(40));
    assert_eq!(known.devices.len(), 1);
    let original = device(&known, "TRACKER-SYNTHETIC-A");
    assert_eq!(original.conn, old_conn);
    assert!(original.host);

    apply(&mut state, 50, remove(3));
    apply(&mut state, 60, add(3));
    apply(&mut state, 70, serial(3, "TRACKER-SYNTHETIC-B"));
    let reused = state.snapshot(LogTime(70));
    let disconnected = device(&reused, "TRACKER-SYNTHETIC-A");
    assert_eq!(
        (
            disconnected.key,
            disconnected.conn,
            disconnected.hub_number,
            disconnected.battery
        ),
        (None, None, None, None),
    );
    assert_eq!(disconnected.connection, Connection::Disconnected);
    assert_eq!(disconnected.phase, Phase::Disconnected);
    assert!(!disconnected.host);
    let replacement = device(&reused, "TRACKER-SYNTHETIC-B");
    assert_eq!(replacement.key, Some(TrackerKey(3)));
    assert_eq!(replacement.conn, Some(tracker(&reused, 3).conn));
    assert_ne!(replacement.conn, old_conn);
    assert!(!replacement.host);
    assert_eq!(reused.host.as_ref().map(|host| host.conn), old_conn);
}

#[test]
fn dongle_reports_and_server_lifecycle_are_independent_of_tracker_phases() {
    let mut state = State::default();
    let initial = state.snapshot(LogTime(0));
    assert_eq!(
        (initial.dongle, initial.hub),
        (DongleState::Unknown, HubState::Unknown)
    );

    apply(&mut state, 10, Event::ServerStarted);
    let running = state.snapshot(LogTime(10));
    assert_eq!(
        (running.dongle, running.hub),
        (DongleState::Unknown, HubState::Running)
    );
    apply(
        &mut state,
        15,
        Event::TrackerAdded {
            tracker: TrackerKey(1),
            usb: true,
        },
    );
    assert_eq!(state.snapshot(LogTime(15)).dongle, DongleState::Unknown);
    for (at, event, expected) in [
        (20, Event::DongleAdded, DongleState::Connected),
        (
            30,
            Event::DongleReport { connected: false },
            DongleState::Disconnected,
        ),
        (
            40,
            Event::DongleReport { connected: true },
            DongleState::Connected,
        ),
        (50, Event::DongleRemoved, DongleState::Disconnected),
        (60, add(2), DongleState::Connected),
    ] {
        apply(&mut state, at, event);
        let snapshot = state.snapshot(LogTime(at));
        assert_eq!(snapshot.dongle, expected);
        assert_eq!(snapshot.hub, HubState::Running);
        assert_eq!(tracker(&snapshot, 1).phase, Phase::Connected);
    }
    let live = state.snapshot(LogTime(60));
    apply(&mut state, 70, Event::DongleReport { connected: false });
    let disconnected = state.snapshot(LogTime(70));
    assert_eq!(disconnected.trackers, live.trackers);
    assert_eq!(disconnected.dongle, DongleState::Disconnected);

    apply(&mut state, 80, Event::ServerStopped);
    let stopped = state.snapshot(LogTime(80));
    assert_eq!(
        (stopped.dongle, stopped.hub),
        (DongleState::Unknown, HubState::Stopped)
    );
    apply(&mut state, 90, add(3));
    apply(&mut state, 100, report(3, true, false, false, None));
    let activity = state.snapshot(LogTime(100));
    assert_eq!(
        (activity.dongle, activity.hub),
        (DongleState::Connected, HubState::Stopped)
    );
}
