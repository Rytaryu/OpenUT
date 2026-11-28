use std::collections::BTreeMap;

use crate::event::Event;
use crate::time::LogTime;
use crate::tracker::Serial;

use super::{
    Connection, DeviceView, DongleState, HubState, Identity, Link, Phase, State, TrackerView,
};

const ROSTER_LIMIT: usize = 16;

impl State {
    pub(super) fn update_device_evidence(&mut self, event: &Event, at: LogTime) {
        match event {
            Event::SerialKnown {
                serial, firmware, ..
            } => {
                let identity = self
                    .roster
                    .entry(serial.clone())
                    .or_insert_with(|| Identity {
                        serial: serial.clone(),
                        firmware: None,
                        last_seen: at,
                    });
                if let Some(firmware) = firmware {
                    identity.firmware = Some(firmware.clone());
                }
                identity.last_seen = at;
                self.trim_roster();
            }
            Event::Status {
                tracker,
                connected: true,
                ..
            } => {
                if let Some(serial) = self
                    .trackers
                    .get(tracker)
                    .filter(|tracker| tracker.view.phase != Phase::Disconnected)
                    .and_then(|tracker| tracker.view.serial.as_ref())
                    && let Some(identity) = self.roster.get_mut(serial)
                {
                    identity.last_seen = at;
                }
            }
            Event::DongleAdded
            | Event::TrackerAdded { usb: false, .. }
            | Event::DongleReport { connected: true } => {
                self.dongle = DongleState::Connected;
            }
            Event::DongleRemoved | Event::DongleReport { connected: false } => {
                self.dongle = DongleState::Disconnected;
            }
            Event::ServerStarted => {
                self.dongle = DongleState::Unknown;
                self.hub = HubState::Running;
            }
            Event::ServerStopped => {
                self.dongle = DongleState::Unknown;
                self.hub = HubState::Stopped;
            }
            _ => {}
        }
    }

    fn trim_roster(&mut self) {
        if self.roster.len() <= ROSTER_LIMIT {
            return;
        }
        let active = self.active_devices();
        // BTreeMap order breaks ties deterministically when timestamps are equal.
        let oldest = self
            .roster
            .iter()
            .min_by_key(|(serial, identity)| (active.contains_key(*serial), identity.last_seen))
            .map(|(serial, _)| serial.clone());
        if let Some(serial) = oldest {
            self.roster.remove(&serial);
        }
    }

    pub(super) fn device_views(&self) -> Vec<DeviceView> {
        let active = self.active_devices();
        let mut devices = Vec::with_capacity(self.roster.len());
        for identity in self.roster.values() {
            devices.push(match active.get(&identity.serial) {
                Some(tracker) => self.connected_device(tracker, identity.firmware.clone()),
                None => DeviceView {
                    serial: Some(identity.serial.clone()),
                    firmware: identity.firmware.clone(),
                    hub_number: None,
                    key: None,
                    conn: None,
                    connection: Connection::Disconnected,
                    battery: None,
                    host: false,
                    phase: Phase::Disconnected,
                },
            });
        }
        devices.extend(
            self.trackers
                .values()
                .map(|tracker| &tracker.view)
                .filter(|view| view.phase != Phase::Disconnected && view.serial.is_none())
                .map(|view| self.connected_device(view, None)),
        );
        devices
    }

    fn active_devices(&self) -> BTreeMap<&Serial, &TrackerView> {
        self.trackers
            .values()
            .filter(|tracker| tracker.view.phase != Phase::Disconnected)
            .filter_map(|tracker| {
                tracker
                    .view
                    .serial
                    .as_ref()
                    .map(|serial| (serial, &tracker.view))
            })
            .collect()
    }

    fn connected_device(&self, tracker: &TrackerView, firmware: Option<String>) -> DeviceView {
        DeviceView {
            serial: tracker.serial.clone(),
            firmware,
            hub_number: tracker.hub_number,
            key: Some(tracker.key),
            conn: Some(tracker.conn),
            connection: match tracker.link {
                Link::Usb => Connection::Usb,
                Link::Dongle | Link::Unknown => Connection::Dongle,
            },
            battery: tracker.battery,
            host: self
                .host
                .as_ref()
                .is_some_and(|host| host.key == tracker.key && host.conn == tracker.conn),
            phase: tracker.phase,
        }
    }
}
