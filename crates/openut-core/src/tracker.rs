//! Tracker identifiers used by the event source and state engine.

use std::collections::BTreeSet;

use crate::rules::{Detection, RuleId};
use crate::state::{ConnId, Connection, DeviceView, Identity, Snapshot};

#[cfg(test)]
mod tests;

const MIN_SERIAL_TAIL: usize = 4;

/// The event source's slot for a tracker during a connection.
/// A slot can be reused after disconnecting.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrackerKey(pub u16);

/// A tracker serial number, when the event source supplies one.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Serial(pub String);

/// The number VIVE Hub shows for the tracker's current connection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HubNumber(pub u16);

/// Tracker identity facts for display or notice formatting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrackerLabel {
    pub hub_number: Option<HubNumber>,
    pub serial_tail: Option<String>,
    pub host: bool,
}

/// The shortest common suffix width that distinguishes the roster entries.
pub(crate) fn serial_tail_length(roster: &[Identity]) -> usize {
    let serials: BTreeSet<_> = roster
        .iter()
        .map(|identity| identity.serial.0.as_str())
        .collect();
    let max_width = serials
        .iter()
        .map(|serial| serial.chars().count())
        .max()
        .unwrap_or_default()
        .max(MIN_SERIAL_TAIL);
    (MIN_SERIAL_TAIL..=max_width)
        .find(|&width| {
            let mut tails = BTreeSet::new();
            serials
                .iter()
                .all(|serial| tails.insert(serial_tail(serial, width)))
        })
        .unwrap_or(max_width)
}

pub(crate) fn device_label(device: &DeviceView, tail_length: usize) -> TrackerLabel {
    let connected = device.connection != Connection::Disconnected;
    let hub_number = device
        .hub_number
        .filter(|_| connected && device.serial.is_some());
    TrackerLabel {
        hub_number,
        serial_tail: device
            .serial
            .as_ref()
            .map(|serial| serial_tail(&serial.0, tail_length).to_owned()),
        host: connected && device.host,
    }
}

/// Freezes a label without identifying a tracker by a reusable slot.
pub(crate) fn target_label(snapshot: &Snapshot, detection: &Detection) -> TrackerLabel {
    label_for(
        snapshot,
        detection.serial.as_ref(),
        detection.conn,
        detection.rule == RuleId::R1,
    )
}

pub(crate) fn host_label(snapshot: &Snapshot) -> TrackerLabel {
    let Some(host) = snapshot.host.as_ref() else {
        return TrackerLabel {
            host: true,
            ..TrackerLabel::default()
        };
    };
    if let Some(device) = snapshot.devices.iter().find(|device| {
        device.conn == Some(host.conn) && device.connection != Connection::Disconnected
    }) {
        let mut label = device_label(device, serial_tail_length(&snapshot.roster));
        label.host = true;
        return label;
    }
    label_for(snapshot, host.serial.as_ref(), None, true)
}

fn label_for(
    snapshot: &Snapshot,
    serial: Option<&Serial>,
    conn: Option<ConnId>,
    host: bool,
) -> TrackerLabel {
    let tail_length = serial_tail_length(&snapshot.roster);
    let matching_device = match serial {
        Some(serial) => snapshot
            .devices
            .iter()
            .find(|device| device.serial.as_ref() == Some(serial)),
        None => conn.and_then(|conn| {
            snapshot
                .devices
                .iter()
                .find(|device| device.conn == Some(conn))
        }),
    };
    if let Some(device) = matching_device {
        let mut label = device_label(device, tail_length);
        label.host |= host;
        return label;
    }
    let tail = serial
        .filter(|serial| {
            snapshot
                .roster
                .iter()
                .any(|identity| &identity.serial == *serial)
        })
        .map(|serial| serial_tail(&serial.0, tail_length).to_owned());
    TrackerLabel {
        hub_number: None,
        serial_tail: tail,
        host,
    }
}

fn serial_tail(serial: &str, length: usize) -> &str {
    serial
        .char_indices()
        .rev()
        .nth(length.max(MIN_SERIAL_TAIL) - 1)
        .map_or(serial, |(start, _)| &serial[start..])
}
