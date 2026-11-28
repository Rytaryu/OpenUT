//! Typed watcher state for status rendering (design §7.4).

mod hub_gate;

#[cfg(test)]
mod tests;

pub use hub_gate::*;

use std::cmp::Reverse;

use crate::event::Battery;
use crate::notice::Notice;
use crate::rules::{RuleId, Step};
use crate::state::{Connection, DeviceView, DongleState, HubState, Identity, Phase, Snapshot};
use crate::tracker::{Serial, TrackerLabel, device_label, serial_tail_length};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchHealth {
    Ok,
    NotFound,
    Unreadable,
    Stalled,
    FormatMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overall {
    Ok,
    Waiting,
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserState {
    Ok,
    Waiting,
    Fault,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Line {
    Fault(FaultRow),
    HubStopped,
    Health(WatchHealth),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultRow {
    pub rule: RuleId,
    pub step: Step,
    pub target: TrackerLabel,
    /// Host identified when the R2 notice was created, if known.
    pub host: Option<TrackerLabel>,
    pub repetitions: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayModel {
    pub overall: Overall,
    pub line: Option<Line>,
    pub faults: Vec<FaultRow>,
    pub dongle: DongleState,
    pub devices: Vec<DeviceRow>,
    pub greyed: bool,
    pub pick_folder: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceRow {
    pub label: TrackerLabel,
    pub serial: Option<Serial>,
    pub connection: Connection,
    pub battery: Option<Battery>,
    pub state: Option<UserState>,
    pub firmware: Option<String>,
    pub host: bool,
}

#[derive(Clone, Copy)]
enum Availability {
    Stopped,
    NotFound,
    Starting,
    CatchingUp,
    Unconfirmed,
    Current,
}

/// Project the watcher snapshot and the latest notices into the same view for every UI.
pub fn view(
    snapshot: &Snapshot,
    live: &[Notice],
    health: WatchHealth,
    hub: HubLive,
    catching_up: bool,
) -> DisplayModel {
    let availability = match (hub, snapshot.hub, health) {
        (HubLive::Stopped, _, _) => Availability::Stopped,
        (_, _, WatchHealth::NotFound) => Availability::NotFound,
        (HubLive::Starting, _, _) | (HubLive::Running, HubState::Stopped, _) => {
            Availability::Starting
        }
        _ if catching_up => Availability::CatchingUp,
        (HubLive::Unknown, HubState::Stopped, _) => Availability::Stopped,
        (_, _, WatchHealth::Ok) => Availability::Current,
        _ => Availability::Unconfirmed,
    };

    let stopped_devices;
    let (devices, notices, dongle): (&[DeviceView], &[Notice], _) = match availability {
        Availability::Stopped | Availability::Starting => {
            stopped_devices = snapshot
                .roster
                .iter()
                .map(disconnected_device)
                .collect::<Vec<_>>();
            (&stopped_devices, &[], DongleState::Unknown)
        }
        Availability::NotFound => (&[], &[], DongleState::Unknown),
        _ => (&snapshot.devices, live, snapshot.dongle),
    };

    let mut sources: Vec<_> = devices.iter().collect();
    sources.sort_by_key(|&device| (device.serial.is_none(), device.serial.as_ref(), device.key));
    let tail_length = serial_tail_length(&snapshot.roster);
    let devices: Vec<_> = sources
        .iter()
        .map(|source| device_row(source, notices, tail_length))
        .collect();
    let mut ordered_notices: Vec<_> = notices
        .iter()
        .filter(|notice| {
            is_active(notice) || notice.rule == RuleId::R4 && notice.step == Step::Resolved
        })
        .collect();
    ordered_notices.sort_by_key(|notice| Reverse((notice.at, notice.episode)));
    let faults: Vec<_> = ordered_notices
        .iter()
        .map(|notice| {
            let target = sources
                .iter()
                .zip(&devices)
                .find(|(source, _)| targets(notice, source))
                .map_or(&notice.target, |(_, row)| &row.label);
            FaultRow {
                rule: notice.rule,
                step: notice.step,
                target: target.clone(),
                host: notice.host.clone(),
                repetitions: notice.repetitions,
            }
        })
        .collect();

    let (overall, line, greyed) = match availability {
        Availability::Stopped => (Overall::Waiting, Some(Line::HubStopped), false),
        Availability::NotFound => (Overall::Waiting, Some(Line::Health(health)), false),
        Availability::Starting => (
            Overall::Waiting,
            (health != WatchHealth::Ok).then_some(Line::Health(health)),
            false,
        ),
        Availability::CatchingUp => (Overall::Waiting, None, true),
        Availability::Unconfirmed => (Overall::Waiting, Some(Line::Health(health)), true),
        Availability::Current => {
            let top_fault = ordered_notices
                .iter()
                .zip(&faults)
                .filter(|(notice, _)| is_active(notice))
                .max_by_key(|(notice, _)| (notice.priority, notice.at, notice.episode));
            if let Some((_, fault)) = top_fault {
                (Overall::Fault, Some(Line::Fault(fault.clone())), false)
            } else if all_connected_tracking(&devices) {
                (Overall::Ok, None, false)
            } else {
                (Overall::Waiting, None, false)
            }
        }
    };

    DisplayModel {
        overall,
        line,
        faults,
        dongle,
        devices,
        greyed,
        pick_folder: health == WatchHealth::NotFound,
    }
}

fn disconnected_device(identity: &Identity) -> DeviceView {
    DeviceView {
        serial: Some(identity.serial.clone()),
        firmware: identity.firmware.clone(),
        hub_number: None,
        key: None,
        conn: None,
        connection: Connection::Disconnected,
        battery: None,
        host: false,
        phase: Phase::Disconnected,
    }
}

fn device_row(source: &DeviceView, live: &[Notice], tail_length: usize) -> DeviceRow {
    let connected = source.connection != Connection::Disconnected;
    let state = if live
        .iter()
        .any(|notice| is_active(notice) && targets(notice, source))
    {
        Some(UserState::Fault)
    } else if !connected {
        None
    } else if source.phase == Phase::Tracking {
        Some(UserState::Ok)
    } else {
        Some(UserState::Waiting)
    };
    DeviceRow {
        label: device_label(source, tail_length),
        serial: source.serial.clone(),
        connection: source.connection,
        battery: source.battery.filter(|_| connected),
        state,
        firmware: source.firmware.clone(),
        host: connected && source.host,
    }
}

fn is_active(notice: &Notice) -> bool {
    matches!(notice.step, Step::Raised | Step::Escalated)
}

fn targets(notice: &Notice, device: &DeviceView) -> bool {
    if let Some(serial) = &notice.serial {
        device.serial.as_ref() == Some(serial)
    } else {
        device.connection != Connection::Disconnected
            && notice.conn.is_some()
            && notice.conn == device.conn
    }
}

fn all_connected_tracking(devices: &[DeviceRow]) -> bool {
    let mut connected = devices
        .iter()
        .filter(|row| row.connection != Connection::Disconnected)
        .peekable();
    connected.peek().is_some() && connected.all(|row| row.state == Some(UserState::Ok))
}
