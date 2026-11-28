use super::{
    ConnId, ConnectionChange, ConnectionChangeKind, DisconnectReason, Link, Phase, Role, State,
    StateUpdate, Tracker, TrackerView,
};
use crate::time::LogTime;
use crate::tracker::TrackerKey;

impl State {
    /// A repeat within the duplicate window is ignored. A later add closes the
    /// old connection and opens a new one at the same instant.
    pub(super) fn add_tracker(
        &mut self,
        key: TrackerKey,
        usb: bool,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        let active = self
            .trackers
            .get_mut(&key)
            .filter(|tracker| tracker.view.phase != Phase::Disconnected);
        let reconnect = active.is_some();
        if let Some(tracker) = active {
            if tracker
                .last_added
                .is_some_and(|last| (0..=self.dup_ms).contains(&at.0.saturating_sub(last.0)))
            {
                return false;
            }
            end_connection(tracker, DisconnectReason::Reconnect, at, update);
        }

        let link = if usb { Link::Usb } else { Link::Dongle };
        self.start_tracker(key, Phase::Connected, link, false, at, update);
        reconnect
    }

    pub(super) fn start_tracker(
        &mut self,
        key: TrackerKey,
        phase: Phase,
        link: Link,
        inferred: bool,
        at: LogTime,
        update: &mut StateUpdate,
    ) {
        let view = TrackerView {
            key,
            serial: None,
            role: Role::Unknown,
            phase,
            since: at,
            conn: ConnId(self.next_conn),
            link,
            tracked_since_connect: phase == Phase::Tracking,
            inferred,
        };
        self.next_conn = self.next_conn.saturating_add(1);
        update.connections.push(ConnectionChange {
            at,
            tracker: view.clone(),
            kind: ConnectionChangeKind::Started,
        });
        self.trackers.insert(
            key,
            Tracker {
                view,
                last_added: (!inferred).then_some(at),
            },
        );
    }

    pub(super) fn remove_tracker(
        &mut self,
        key: TrackerKey,
        reason: DisconnectReason,
        at: LogTime,
        update: &mut StateUpdate,
    ) {
        if let Some(tracker) = self.trackers.get_mut(&key) {
            end_connection(tracker, reason, at, update);
        }
    }

    pub(super) fn disconnect_trackers(
        &mut self,
        reason: DisconnectReason,
        at: LogTime,
        update: &mut StateUpdate,
    ) {
        for tracker in self.trackers.values_mut() {
            if reason != DisconnectReason::DongleRemoved || tracker.view.link != Link::Usb {
                end_connection(tracker, reason, at, update);
            }
        }
    }
}

fn end_connection(
    tracker: &mut Tracker,
    reason: DisconnectReason,
    at: LogTime,
    update: &mut StateUpdate,
) {
    let view = &mut tracker.view;
    if view.phase == Phase::Disconnected {
        return;
    }

    update.connections.push(ConnectionChange {
        at,
        tracker: view.clone(),
        kind: ConnectionChangeKind::Ended(reason),
    });
    view.phase = Phase::Disconnected;
    view.since = at;
    view.role = Role::Unknown;
    view.serial = None;
    view.link = Link::Unknown;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, PoseStatus, Timed};
    use crate::time::UtcOffset;
    use crate::tracker::Serial;

    fn apply(state: &mut State, at: i64, event: Event) -> StateUpdate {
        state.apply(&Timed {
            at: LogTime(at),
            offset: UtcOffset(0),
            event,
        })
    }

    fn add(state: &mut State, at: i64, tracker: TrackerKey, usb: bool) -> StateUpdate {
        apply(state, at, Event::TrackerAdded { tracker, usb })
    }

    #[test]
    fn adds_removes_and_starts_a_fresh_connection_for_the_same_slot() {
        let mut state = State::default();
        let key = TrackerKey(2);
        let started = add(&mut state, 10, key, false);
        let [started] = started.connections.as_slice() else {
            panic!("first add should create one connection");
        };
        assert_eq!(started.kind, ConnectionChangeKind::Started);
        assert_eq!(started.at, LogTime(10));
        assert_eq!(started.tracker.conn, ConnId(1));
        assert_eq!(started.tracker.phase, Phase::Connected);
        assert_eq!(started.tracker.link, Link::Dongle);
        assert!(!started.tracker.inferred);
        assert!(!started.tracker.tracked_since_connect);

        let serial = Serial("TRACKER-TEST".to_owned());
        apply(&mut state, 12, Event::HostAssigned { tracker: key });
        apply(
            &mut state,
            13,
            Event::SerialKnown {
                tracker: key,
                serial: serial.clone(),
            },
        );
        let removed = apply(
            &mut state,
            20,
            Event::TrackerRemoved {
                tracker: key,
                usb: false,
            },
        );
        let [removed] = removed.connections.as_slice() else {
            panic!("removal should end one connection");
        };
        assert_eq!(removed.at, LogTime(20));
        assert_eq!(
            removed.kind,
            ConnectionChangeKind::Ended(DisconnectReason::TrackerRemoved),
        );
        assert_eq!(removed.tracker.conn, started.tracker.conn);
        assert_eq!(removed.tracker.serial, Some(serial));
        assert_eq!(removed.tracker.role, Role::Host);

        let duplicate = apply(
            &mut state,
            23,
            Event::TrackerRemoved {
                tracker: key,
                usb: false,
            },
        );
        assert!(duplicate.connections.is_empty());
        assert!(!duplicate.out_of_order);
        let snapshot = state.snapshot(LogTime(23));
        let view = &snapshot.trackers[0];
        assert_eq!(view.phase, Phase::Disconnected);
        assert_eq!(view.conn, started.tracker.conn);
        assert_eq!(view.since, LogTime(20));
        assert_eq!(view.role, Role::Unknown);
        assert_eq!(view.serial, None);
        assert_eq!(view.link, Link::Unknown);

        let connected_again = add(&mut state, 25, key, true);
        let [connected_again] = connected_again.connections.as_slice() else {
            panic!("a new add should start one connection");
        };
        assert_eq!(connected_again.tracker.link, Link::Usb);
        assert_eq!(connected_again.tracker.conn, ConnId(2));
        assert_eq!(connected_again.tracker.since, LogTime(25));
    }

    #[test]
    fn duplicate_window_has_an_inclusive_boundary_and_reconnect_closes_then_starts() {
        let mut state = State::default();
        let key = TrackerKey(3);
        add(&mut state, 100, key, false);
        apply(&mut state, 101, Event::HostAssigned { tracker: key });
        let original = state.snapshot(LogTime(101));

        let duplicate = add(&mut state, 1_100, key, true);
        assert!(duplicate.connections.is_empty());
        assert!(!duplicate.out_of_order);
        assert_eq!(state.snapshot(LogTime(1_100)).trackers, original.trackers);

        let reconnected = add(&mut state, 1_101, key, true);
        assert!(reconnected.out_of_order);
        let [ended, started] = reconnected.connections.as_slice() else {
            panic!("reconnect should end one connection and start another");
        };
        assert_eq!(
            ended.kind,
            ConnectionChangeKind::Ended(DisconnectReason::Reconnect),
        );
        assert_eq!(started.kind, ConnectionChangeKind::Started);
        assert_eq!(ended.at, LogTime(1_101));
        assert_eq!(ended.at, started.at);
        assert_eq!(ended.tracker.conn, original.trackers[0].conn);
        assert_ne!(started.tracker.conn, ended.tracker.conn);
        assert_eq!(started.tracker.phase, Phase::Connected);
        assert_eq!(started.tracker.role, Role::Unknown);
        assert_eq!(started.tracker.link, Link::Usb);
        assert_eq!(started.tracker.serial, None);
    }

    #[test]
    fn explicit_add_ends_an_inferred_connection_even_within_the_duplicate_window() {
        let mut state = State::default();
        let key = TrackerKey(4);
        apply(
            &mut state,
            10,
            Event::Pose {
                tracker: key,
                status: PoseStatus::Ok,
            },
        );
        let result = add(&mut state, 11, key, false);
        assert!(result.out_of_order);
        let [ended, started] = result.connections.as_slice() else {
            panic!("the inferred connection should be replaced");
        };
        assert_eq!(
            ended.kind,
            ConnectionChangeKind::Ended(DisconnectReason::Reconnect),
        );
        assert!(ended.tracker.inferred);
        assert!(ended.tracker.tracked_since_connect);
        assert_eq!(started.kind, ConnectionChangeKind::Started);
        assert_eq!(started.tracker.phase, Phase::Connected);
        assert!(!started.tracker.inferred);
        assert!(!started.tracker.tracked_since_connect);
    }

    #[test]
    fn dongle_removal_ends_dongle_and_unknown_paths_without_changing_usb() {
        let mut state = State::default();
        let usb = TrackerKey(1);
        let dongle = TrackerKey(2);
        let unknown = TrackerKey(3);
        add(&mut state, 5, usb, true);
        add(&mut state, 6, dongle, false);
        apply(
            &mut state,
            7,
            Event::Pose {
                tracker: unknown,
                status: PoseStatus::Ok,
            },
        );
        apply(&mut state, 8, Event::HostAssigned { tracker: usb });
        apply(
            &mut state,
            9,
            Event::Pose {
                tracker: usb,
                status: PoseStatus::Ok,
            },
        );
        apply(
            &mut state,
            9,
            Event::SerialKnown {
                tracker: usb,
                serial: Serial("TRACKER-USB".to_owned()),
            },
        );
        let before = state.snapshot(LogTime(9));
        let result = apply(&mut state, 10, Event::DongleRemoved);
        assert_eq!(
            result
                .connections
                .iter()
                .map(|change| (change.tracker.key, change.kind))
                .collect::<Vec<_>>(),
            [
                (
                    dongle,
                    ConnectionChangeKind::Ended(DisconnectReason::DongleRemoved),
                ),
                (
                    unknown,
                    ConnectionChangeKind::Ended(DisconnectReason::DongleRemoved),
                ),
            ],
        );
        let after = state.snapshot(LogTime(10));
        assert_eq!(after.trackers[0], before.trackers[0]);
        assert_eq!(after.trackers[1].phase, Phase::Disconnected);
        assert_eq!(after.trackers[2].phase, Phase::Disconnected);
    }

    #[test]
    fn either_server_boundary_ends_all_active_connections() {
        for (event, reason) in [
            (Event::ServerStarted, DisconnectReason::ServerStarted),
            (Event::ServerStopped, DisconnectReason::ServerStopped),
        ] {
            let mut state = State::default();
            add(&mut state, 1, TrackerKey(2), false);
            add(&mut state, 2, TrackerKey(1), true);
            let result = apply(&mut state, 10, event);
            assert_eq!(result.connections.len(), 2);
            assert!(result.connections.iter().all(|change| {
                change.at == LogTime(10) && change.kind == ConnectionChangeKind::Ended(reason)
            }));
            assert!(state.snapshot(LogTime(10)).trackers.iter().all(|tracker| {
                tracker.phase == Phase::Disconnected && tracker.link == Link::Unknown
            }));
        }
    }
}
