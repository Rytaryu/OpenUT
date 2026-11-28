use crate::event::Event;
use crate::time::LogTime;
use crate::tracker::TrackerKey;

use super::{HostBy, HostRef, HostWifi, Phase, Role, State, TrackerView};

impl State {
    /// Update the PC's Wi-Fi evidence independently of the host's identity.
    pub(super) fn update_wifi(&mut self, event: &Event, at: LogTime) -> bool {
        match event {
            Event::ServerStarted | Event::HostAssigned { .. } => {
                // Even a repeated command starts a new attempt to obtain Wi-Fi info.
                self.host_wifi = HostWifi::Missing {
                    since: at,
                    queried: false,
                };
            }
            Event::ServerStopped => self.host_wifi = HostWifi::Unknown,
            Event::HostWifiQueried { .. } => {
                if let HostWifi::Missing { queried, .. } = &mut self.host_wifi {
                    *queried = true;
                }
            }
            Event::HostWifiObtained { .. } | Event::WifiAssigned { .. }
                if !matches!(self.host_wifi, HostWifi::Held { .. }) =>
            {
                self.host_wifi = HostWifi::Held { since: at };
                return matches!(event, Event::WifiAssigned { .. });
            }
            Event::WifiRequestUnanswered { .. } => {
                return matches!(self.host_wifi, HostWifi::Held { .. });
            }
            _ => {}
        }
        false
    }

    /// Apply host evidence after the tracker rows have been updated.
    pub(super) fn update_host(&mut self, event: &Event, at: LogTime) -> bool {
        match *event {
            Event::HostAssigned { tracker } => {
                if !self.remember_host(tracker, HostBy::Command, at) {
                    self.clear_host();
                    return true;
                }
            }
            Event::Status {
                tracker,
                connected: true,
                host: true,
                ..
            } => {
                if self.live_command_host().is_none() {
                    self.remember_host(tracker, HostBy::Report, at);
                }
            }
            Event::HostWifiQueried { host } => {
                if let Some(command_host) = self.live_command_host() {
                    return command_host != host;
                }
                return !self.remember_host(host, HostBy::Report, at);
            }
            Event::SerialKnown {
                tracker,
                ref serial,
            } => {
                let connection = self.live_tracker(tracker).map(|view| view.conn);
                if let Some(host) = &mut self.host
                    && host.key == tracker
                    && Some(host.conn) == connection
                {
                    host.serial = Some(serial.clone());
                }
            }
            Event::ServerStarted | Event::ServerStopped => self.clear_host(),
            // Disconnects preserve the evidence for the old connection.
            _ => {}
        }
        false
    }

    fn live_tracker(&self, key: TrackerKey) -> Option<&TrackerView> {
        self.trackers
            .get(&key)
            .map(|tracker| &tracker.view)
            .filter(|view| view.phase != Phase::Disconnected)
    }

    fn live_command_host(&self) -> Option<TrackerKey> {
        let host = self.host.as_ref()?;
        (host.by == HostBy::Command && self.live_tracker(host.key)?.conn == host.conn)
            .then_some(host.key)
    }

    fn remember_host(&mut self, key: TrackerKey, by: HostBy, at: LogTime) -> bool {
        let Some(view) = self.live_tracker(key) else {
            return false;
        };
        if self
            .host
            .as_ref()
            .is_some_and(|host| host.key == key && host.conn == view.conn && host.by == by)
        {
            return true;
        }
        let next = HostRef {
            key,
            conn: view.conn,
            serial: view.serial.clone(),
            by,
            since: at,
        };
        self.clear_host();
        if let Some(tracker) = self.trackers.get_mut(&key) {
            tracker.view.role = Role::Host;
        }
        self.host = Some(next);
        true
    }

    fn clear_host(&mut self) {
        if let Some(host) = self.host.take()
            && let Some(tracker) = self.trackers.get_mut(&host.key)
            && tracker.view.conn == host.conn
            && tracker.view.role == Role::Host
        {
            tracker.view.role = Role::Unknown;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::event::Timed;
    use crate::time::UtcOffset;
    use crate::tracker::Serial;

    use super::*;

    fn apply(state: &mut State, ms: i64, event: Event) -> bool {
        state
            .apply(&Timed {
                at: LogTime(ms),
                offset: UtcOffset(0),
                event,
            })
            .out_of_order
    }

    fn added(tracker: u16) -> Event {
        Event::TrackerAdded {
            tracker: TrackerKey(tracker),
            usb: false,
        }
    }

    fn command(tracker: u16) -> Event {
        Event::HostAssigned {
            tracker: TrackerKey(tracker),
        }
    }

    fn status(tracker: u16, connected: bool) -> Event {
        Event::Status {
            tracker: TrackerKey(tracker),
            connected,
            host: true,
            map_ready: false,
            usb: None,
        }
    }

    fn queried(host: u16) -> Event {
        Event::HostWifiQueried {
            host: TrackerKey(host),
        }
    }

    #[test]
    fn host_wifi_tracks_the_current_attempt_without_replacing_the_host() {
        let mut state = State::default();
        assert_eq!(state.host_wifi, HostWifi::Unknown);
        assert!(!apply(&mut state, 10, Event::ServerStarted));
        assert_eq!(
            state.host_wifi,
            HostWifi::Missing {
                since: LogTime(10),
                queried: false,
            }
        );
        assert!(!apply(&mut state, 15, queried(3)));
        assert_eq!(
            state.host_wifi,
            HostWifi::Missing {
                since: LogTime(10),
                queried: true,
            }
        );
        assert!(matches!(
            state.host.as_ref(),
            Some(HostRef {
                key: TrackerKey(3),
                by: HostBy::Report,
                ..
            })
        ));

        assert!(!apply(&mut state, 20, command(3)));
        let obtained = Event::HostWifiObtained { host: None };
        assert!(!apply(&mut state, 25, obtained.clone()));
        assert!(!apply(&mut state, 26, queried(3)));
        assert!(!apply(&mut state, 28, obtained));
        assert_eq!(state.host_wifi, HostWifi::Held { since: LogTime(25) });
        assert_eq!(
            state.host.as_ref().map(|host| host.by),
            Some(HostBy::Command)
        );
        assert!(!apply(&mut state, 30, command(3)));
        assert_eq!(
            state.host_wifi,
            HostWifi::Missing {
                since: LogTime(30),
                queried: false,
            }
        );
        assert_eq!(
            state.host.as_ref().map(|host| host.since),
            Some(LogTime(20))
        );
    }

    #[test]
    fn wifi_assignment_and_unanswered_request_report_conflicting_wifi_evidence() {
        let mut state = State::default();
        let unanswered = Event::WifiRequestUnanswered { tracker: None };
        assert!(!apply(&mut state, 1, unanswered.clone()));
        assert_eq!(state.host_wifi, HostWifi::Unknown);
        apply(&mut state, 2, added(1));
        let assigned = Event::WifiAssigned {
            tracker: TrackerKey(1),
        };
        assert!(apply(&mut state, 3, assigned.clone()));
        assert!(!apply(&mut state, 4, assigned));
        assert!(apply(&mut state, 5, unanswered));
        assert_eq!(state.host_wifi, HostWifi::Held { since: LogTime(3) });
    }

    #[test]
    fn command_replaces_report_and_only_conflicting_queries_count() {
        let mut state = State::default();
        assert!(!apply(&mut state, 1, status(3, false)));
        assert!(!apply(&mut state, 2, status(2, true)));
        assert_eq!(state.host_wifi, HostWifi::Unknown);
        assert_eq!(
            state.host.as_ref().map(|host| host.key),
            Some(TrackerKey(2))
        );
        apply(&mut state, 3, added(0));
        assert!(!apply(&mut state, 4, command(0)));
        assert_eq!(state.trackers[&TrackerKey(2)].view.role, Role::Unknown);
        assert!(!apply(&mut state, 5, status(2, true)));
        assert!(!apply(&mut state, 6, queried(0)));
        assert!(apply(&mut state, 7, queried(3)));
        assert_eq!(state.trackers[&TrackerKey(0)].view.role, Role::Host);
        assert_eq!(
            state.host.as_ref().map(|host| host.key),
            Some(TrackerKey(0))
        );
        assert_eq!(state.host.as_ref().map(|host| host.since), Some(LogTime(4)));
    }

    #[test]
    fn reused_slot_does_not_replace_serial_evidence_for_the_ended_host_connection() {
        let mut state = State::default();
        let tracker = TrackerKey(2);
        let serial_a = Serial("SYNTHETIC-A".into());
        let serial_b = Serial("SYNTHETIC-B".into());
        let removed = Event::TrackerRemoved {
            tracker,
            usb: false,
        };
        apply(&mut state, 1, added(2));
        apply(&mut state, 2, command(2));
        apply(
            &mut state,
            3,
            Event::SerialKnown {
                tracker,
                serial: serial_a.clone(),
            },
        );
        let original = state.trackers[&tracker].view.conn;
        apply(&mut state, 4, Event::HostWifiObtained { host: None });
        apply(&mut state, 5, removed);
        apply(&mut state, 6, added(2));
        apply(
            &mut state,
            7,
            Event::SerialKnown {
                tracker,
                serial: serial_b.clone(),
            },
        );
        let reused = &state.trackers[&tracker].view;
        let remembered = state.host.as_ref().expect("ended host evidence remains");
        assert_eq!(remembered.conn, original);
        assert_eq!(remembered.serial.as_ref(), Some(&serial_a));
        assert_ne!(reused.conn, original);
        assert_eq!(reused.role, Role::Unknown);

        apply(&mut state, 8, status(2, true));
        let current = state
            .host
            .as_ref()
            .expect("a report replaced an ended host");
        assert_eq!(current.conn, state.trackers[&tracker].view.conn);
        assert_eq!(current.by, HostBy::Report);
        assert_eq!(current.serial.as_ref(), Some(&serial_b));
        assert_eq!(state.trackers[&tracker].view.role, Role::Host);
        apply(&mut state, 9, Event::DongleRemoved);
        assert_eq!(state.trackers[&tracker].view.role, Role::Unknown);
        assert_eq!(
            state.host.as_ref().and_then(|host| host.serial.as_ref()),
            Some(&serial_b)
        );
        assert_eq!(state.host_wifi, HostWifi::Held { since: LogTime(4) });
    }

    #[test]
    fn invalid_host_command_and_server_boundaries_clear_the_host() {
        let mut state = State::default();
        apply(&mut state, 1, added(1));
        apply(&mut state, 2, added(2));
        apply(&mut state, 3, command(1));
        apply(
            &mut state,
            4,
            Event::TrackerRemoved {
                tracker: TrackerKey(2),
                usb: false,
            },
        );
        assert!(apply(&mut state, 5, command(2)));
        assert!(state.host.is_none());
        assert_eq!(state.trackers[&TrackerKey(1)].view.role, Role::Unknown);

        apply(&mut state, 6, command(1));
        apply(&mut state, 7, Event::ServerStarted);
        assert!(state.host.is_none());
        assert_eq!(
            state.host_wifi,
            HostWifi::Missing {
                since: LogTime(7),
                queried: false,
            }
        );
        apply(&mut state, 8, added(1));
        apply(&mut state, 9, command(1));
        apply(&mut state, 10, Event::ServerStopped);
        assert!(state.host.is_none());
        assert_eq!(state.host_wifi, HostWifi::Unknown);
    }
}
