use super::{
    HostBy, HostWifi, Link, Phase, Role, Snapshot, State, StateTally, StateUpdate, TrackerView,
};
use crate::{
    event::{Event, OetKind, PoseStatus, Timed},
    time::{LogTime, UtcOffset},
    tracker::{Serial, TrackerKey},
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
    Event::SerialKnown {
        tracker: TrackerKey(slot),
        serial: Serial(value.into()),
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
