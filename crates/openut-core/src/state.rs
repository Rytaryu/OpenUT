//! Tracker, host, and Wi-Fi state derived from events (watcher design §5).

mod connection;
mod devices;
mod host;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::event::{Battery, Event, OetKind, PoseStatus, Timed};
use crate::time::LogTime;
use crate::tracker::{HubNumber, Serial, TrackerKey};

pub(crate) const DEFAULT_DUP_MS: i64 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Disconnected,
    Connected,
    WaitingWifi,
    MapSyncing,
    Relocalizing,
    Tracking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Unknown,
    Host,
    Client,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Link {
    Dongle,
    Usb,
    Unknown,
}

/// A connection of a slot. IDs start at one and are retained until state is rebuilt.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerView {
    pub key: TrackerKey,
    pub serial: Option<Serial>,
    pub role: Role,
    pub phase: Phase,
    pub since: LogTime,
    pub conn: ConnId,
    pub link: Link,
    pub tracked_since_connect: bool,
    pub inferred: bool,
    pub battery: Option<Battery>,
    pub hub_number: Option<HubNumber>,
}

impl TrackerView {
    fn set_phase(&mut self, phase: Phase, at: LogTime) {
        if self.phase != phase {
            self.phase = phase;
            self.since = at;
            self.tracked_since_connect |= phase == Phase::Tracking;
        }
    }

    fn set_client(&mut self) {
        // The live HostRef determines the host role until its evidence changes.
        if self.role != Role::Host {
            self.role = Role::Client;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostBy {
    Command,
    Report,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostRef {
    pub key: TrackerKey,
    pub conn: ConnId,
    pub serial: Option<Serial>,
    pub by: HostBy,
    pub since: LogTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostWifi {
    Unknown,
    Missing { since: LogTime, queried: bool },
    Held { since: LogTime },
}

/// The last information reported for a tracker, independent of its connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity {
    pub serial: Serial,
    pub firmware: Option<String>,
    pub last_seen: LogTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DongleState {
    Unknown,
    Connected,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubState {
    Unknown,
    Running,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Connection {
    Disconnected,
    Dongle,
    Usb,
}

/// One roster entry or active tracker whose serial is not yet known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceView {
    pub serial: Option<Serial>,
    pub firmware: Option<String>,
    pub hub_number: Option<HubNumber>,
    pub key: Option<TrackerKey>,
    pub conn: Option<ConnId>,
    pub connection: Connection,
    pub battery: Option<Battery>,
    pub host: bool,
    pub phase: Phase,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub trackers: Vec<TrackerView>,
    pub host: Option<HostRef>,
    pub host_wifi: HostWifi,
    pub roster: Vec<Identity>,
    pub devices: Vec<DeviceView>,
    pub dongle: DongleState,
    pub hub: HubState,
    pub at: LogTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    TrackerRemoved,
    Reconnect,
    DongleRemoved,
    ServerStarted,
    ServerStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionChangeKind {
    Started,
    Ended(DisconnectReason),
}

/// For an ended connection, preserves its last serial, role, path and phase.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionChange {
    pub at: LogTime,
    pub tracker: TrackerView,
    pub kind: ConnectionChangeKind,
}

/// Inputs counted by the summary without requiring a phase change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateTally {
    ClientLinkLost,
    OtherPose,
    Overspeed,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct StateUpdate {
    pub connections: Vec<ConnectionChange>,
    pub out_of_order: bool,
    pub tally: Option<StateTally>,
}

struct Tracker {
    view: TrackerView,
    last_added: Option<LogTime>,
}

struct TrackerStart {
    phase: Phase,
    link: Link,
    inferred: bool,
    battery: Option<Battery>,
}

/// The state transition table. Time and event order come from the caller.
pub struct State {
    trackers: BTreeMap<TrackerKey, Tracker>,
    host: Option<HostRef>,
    host_wifi: HostWifi,
    roster: BTreeMap<Serial, Identity>,
    dongle: DongleState,
    hub: HubState,
    next_conn: u64,
    dup_ms: i64,
    out_of_order: u64,
}

impl Default for State {
    fn default() -> Self {
        Self::new(DEFAULT_DUP_MS)
    }
}

impl State {
    pub fn new(dup_ms: i64) -> Self {
        Self {
            trackers: BTreeMap::new(),
            host: None,
            host_wifi: HostWifi::Unknown,
            roster: BTreeMap::new(),
            dongle: DongleState::Unknown,
            hub: HubState::Unknown,
            next_conn: 1,
            dup_ms: dup_ms.max(0),
            out_of_order: 0,
        }
    }

    pub fn snapshot(&self, at: LogTime) -> Snapshot {
        Snapshot {
            trackers: self
                .trackers
                .values()
                .map(|tracker| tracker.view.clone())
                .collect(),
            host: self.host.clone(),
            host_wifi: self.host_wifi,
            roster: self.roster.values().cloned().collect(),
            devices: self.device_views(),
            dongle: self.dongle,
            hub: self.hub,
            at,
        }
    }

    pub(crate) fn tracker(&self, key: TrackerKey) -> Option<&TrackerView> {
        self.trackers.get(&key).map(|tracker| &tracker.view)
    }

    pub(crate) fn trackers(&self) -> impl Iterator<Item = &TrackerView> {
        self.trackers.values().map(|tracker| &tracker.view)
    }

    pub(crate) fn host(&self) -> Option<&HostRef> {
        self.host.as_ref()
    }

    pub(crate) fn host_wifi(&self) -> HostWifi {
        self.host_wifi
    }

    pub fn out_of_order(&self) -> u64 {
        self.out_of_order
    }

    pub fn apply(&mut self, timed: &Timed) -> StateUpdate {
        let mut update = StateUpdate::default();
        let event = &timed.event;
        let at = timed.at;
        let tracker_out_of_order = self.update_trackers(event, at, &mut update);
        let wifi_out_of_order = self.update_wifi(event, at);
        let host_out_of_order = self.update_host(event, at);
        self.update_device_evidence(event, at);
        update.out_of_order = tracker_out_of_order || wifi_out_of_order || host_out_of_order;
        self.out_of_order = self
            .out_of_order
            .saturating_add(u64::from(update.out_of_order));
        update
    }

    fn update_trackers(&mut self, event: &Event, at: LogTime, update: &mut StateUpdate) -> bool {
        match *event {
            Event::TrackerAdded { tracker, usb } => self.add_tracker(tracker, usb, at, update),
            Event::TrackerRemoved { tracker, .. } => {
                self.remove_tracker(tracker, DisconnectReason::TrackerRemoved, at, update);
                false
            }
            Event::DongleRemoved | Event::ServerStarted | Event::ServerStopped => {
                let reason = match event {
                    Event::DongleRemoved => DisconnectReason::DongleRemoved,
                    Event::ServerStarted => DisconnectReason::ServerStarted,
                    _ => DisconnectReason::ServerStopped,
                };
                self.disconnect_trackers(reason, at, update);
                false
            }
            Event::SerialKnown {
                tracker,
                ref serial,
                hub_number,
                ..
            } => self.set_serial(tracker, serial, hub_number),
            Event::HostAssigned { tracker } => self.assign_host_phase(tracker, at, update),
            Event::HostWifiQueried { host } => {
                self.infer_if_missing(host, Phase::Connected, at, update);
                false
            }
            Event::WifiAssigned { tracker } => self.assign_client_wifi(tracker, at, update),
            Event::WifiRequestUnanswered { .. } => {
                self.note_unanswered_wifi(at);
                false
            }
            Event::ClientLink { tracker, connected } => {
                self.client_link(tracker, connected, at, update)
            }
            Event::Status {
                tracker,
                connected,
                map_ready,
                usb,
                battery,
                ..
            } => self.status(
                tracker,
                StatusReport {
                    connected,
                    map_ready,
                    usb,
                    battery,
                },
                at,
                update,
            ),
            Event::Pose { tracker, status } => {
                let motion = match status {
                    PoseStatus::Ok => Motion::Tracked,
                    PoseStatus::Lost | PoseStatus::RecentlyLost => Motion::Lost { pose: true },
                    PoseStatus::Other(_) => Motion::Tally(StateTally::OtherPose),
                };
                self.motion(tracker, motion, at, update)
            }
            Event::Oet { tracker, kind } => {
                let motion = match kind {
                    OetKind::RelocBack => Motion::Tracked,
                    OetKind::LostTracking => Motion::Lost { pose: false },
                    OetKind::Overspeed => Motion::Tally(StateTally::Overspeed),
                };
                self.motion(tracker, motion, at, update)
            }
            Event::DongleAdded
            | Event::DongleReport { .. }
            | Event::HostWifiObtained { .. }
            | Event::PowerOffNotice { .. }
            | Event::PcCommand { .. } => false,
        }
    }

    fn infer_if_missing(
        &mut self,
        key: TrackerKey,
        phase: Phase,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        if self.trackers.contains_key(&key) {
            return false;
        }
        self.start_tracker(
            key,
            TrackerStart {
                phase,
                link: Link::Unknown,
                inferred: true,
                battery: None,
            },
            at,
            update,
        );
        true
    }

    fn set_serial(
        &mut self,
        key: TrackerKey,
        serial: &Serial,
        hub_number: Option<HubNumber>,
    ) -> bool {
        if !self
            .trackers
            .get(&key)
            .is_some_and(|tracker| tracker.view.phase != Phase::Disconnected)
        {
            return true;
        }
        for tracker in self.trackers.values_mut() {
            let view = &mut tracker.view;
            if view.key == key {
                if hub_number.is_some() || view.serial.as_ref() != Some(serial) {
                    view.hub_number = hub_number;
                }
                view.serial = Some(serial.clone());
            } else if view.serial.as_ref() == Some(serial) {
                view.serial = None;
                view.hub_number = None;
            }
        }
        false
    }

    fn assign_host_phase(
        &mut self,
        key: TrackerKey,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        self.infer_if_missing(key, Phase::MapSyncing, at, update);
        let Some(tracker) = self.trackers.get_mut(&key) else {
            return true;
        };
        match tracker.view.phase {
            Phase::Disconnected => return true,
            Phase::Connected | Phase::WaitingWifi => tracker.view.set_phase(Phase::MapSyncing, at),
            _ => {}
        }
        for tracker in self.trackers.values_mut() {
            if tracker.view.key != key && tracker.view.phase == Phase::Connected {
                tracker.view.set_phase(Phase::WaitingWifi, at);
            }
        }
        false
    }

    fn assign_client_wifi(
        &mut self,
        key: TrackerKey,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        self.infer_if_missing(key, Phase::WaitingWifi, at, update);
        let Some(view) = self.trackers.get_mut(&key).map(|tracker| &mut tracker.view) else {
            return true;
        };
        if matches!(view.phase, Phase::Connected | Phase::WaitingWifi) {
            view.set_phase(Phase::WaitingWifi, at);
            view.set_client();
            return false;
        }
        true
    }

    fn note_unanswered_wifi(&mut self, at: LogTime) {
        let mut candidates = self.trackers.values_mut().filter(|tracker| {
            let view = &tracker.view;
            view.role != Role::Host && matches!(view.phase, Phase::Connected | Phase::WaitingWifi)
        });
        if let Some(tracker) = candidates.next()
            && candidates.next().is_none()
        {
            tracker.view.set_phase(Phase::WaitingWifi, at);
            tracker.view.set_client();
        }
    }

    fn client_link(
        &mut self,
        key: TrackerKey,
        connected: bool,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        let initial = if connected {
            Phase::MapSyncing
        } else {
            Phase::Connected
        };
        let inferred = self.infer_if_missing(key, initial, at, update);
        let Some(view) = self.trackers.get_mut(&key).map(|tracker| &mut tracker.view) else {
            return true;
        };
        match (connected, view.phase) {
            (true, Phase::Connected | Phase::WaitingWifi | Phase::MapSyncing) => {
                view.set_phase(Phase::MapSyncing, at);
                view.set_client();
            }
            (false, Phase::MapSyncing) => view.set_phase(Phase::WaitingWifi, at),
            (false, Phase::Relocalizing | Phase::Tracking) => {
                update.tally = Some(StateTally::ClientLinkLost)
            }
            _ => return !inferred,
        }
        false
    }

    fn status(
        &mut self,
        key: TrackerKey,
        report: StatusReport,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        let StatusReport {
            connected,
            map_ready,
            usb,
            battery,
        } = report;
        let reported_link = usb.map_or(
            Link::Unknown,
            |usb| if usb { Link::Usb } else { Link::Dongle },
        );
        if let Some(tracker) = self.trackers.get_mut(&key) {
            let view = &mut tracker.view;
            if view.phase == Phase::Disconnected {
                return false;
            }
            if view.link == Link::Unknown {
                view.link = reported_link;
            }
            if !connected {
                return true;
            }
            if let Some(battery) = battery {
                view.battery = Some(battery);
            }
            if map_ready
                && matches!(
                    view.phase,
                    Phase::Connected | Phase::WaitingWifi | Phase::MapSyncing
                )
            {
                view.set_phase(Phase::Relocalizing, at);
            }
        } else if connected {
            let phase = if map_ready {
                Phase::Relocalizing
            } else {
                Phase::Connected
            };
            self.start_tracker(
                key,
                TrackerStart {
                    phase,
                    link: reported_link,
                    inferred: true,
                    battery,
                },
                at,
                update,
            );
        }
        false
    }

    fn motion(
        &mut self,
        key: TrackerKey,
        motion: Motion,
        at: LogTime,
        update: &mut StateUpdate,
    ) -> bool {
        let inferred_phase = match motion {
            Motion::Tracked => Phase::Tracking,
            Motion::Lost { .. } => Phase::Relocalizing,
            Motion::Tally(_) => Phase::Connected,
        };
        self.infer_if_missing(key, inferred_phase, at, update);
        let Some(view) = self.trackers.get_mut(&key).map(|tracker| &mut tracker.view) else {
            return true;
        };
        if view.phase == Phase::Disconnected {
            return true;
        }
        let early = matches!(view.phase, Phase::Connected | Phase::WaitingWifi);
        match motion {
            Motion::Tracked => view.set_phase(Phase::Tracking, at),
            Motion::Lost { .. }
                if matches!(
                    view.phase,
                    Phase::Tracking | Phase::MapSyncing | Phase::Relocalizing
                ) =>
            {
                view.set_phase(Phase::Relocalizing, at)
            }
            Motion::Lost { pose: true } if early => {}
            Motion::Lost { .. } => return true,
            Motion::Tally(StateTally::OtherPose) if early => {}
            Motion::Tally(tally) => update.tally = Some(tally),
        }
        false
    }
}

struct StatusReport {
    connected: bool,
    map_ready: bool,
    usb: Option<bool>,
    battery: Option<Battery>,
}

#[derive(Clone, Copy)]
enum Motion {
    Tracked,
    Lost { pose: bool },
    Tally(StateTally),
}
