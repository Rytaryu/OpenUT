//! User-facing view of the watcher state (design §7.4).

mod hub_gate;

#[cfg(test)]
mod tests;

pub use hub_gate::*;

use std::cmp::Reverse;

use crate::event::Battery;
use crate::notice::Notice;
use crate::rules::{RuleId, Step};
use crate::state::{Connection, DeviceView, DongleState, HubState, Identity, Phase, Snapshot};
use crate::text::{TrackerLabel, device_label, fill_notice, serial_tail_length};
use crate::tracker::Serial;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuideStepId {
    G1,
    G2,
    G3,
    G4,
    G5,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuideStep {
    pub id: GuideStepId,
    pub done: bool,
    pub progress: Option<(u16, u16)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Line {
    Fault { headline: String, action: String },
    Guide(GuideStepId),
    Health(WatchHealth),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultRow {
    pub headline: String,
    pub action: String,
    pub resolved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayModel {
    pub overall: Overall,
    pub line: Option<Line>,
    pub faults: Vec<FaultRow>,
    /// Setup progress for the watcher. The production UI follows D-46 and omits a checklist.
    pub guide: Vec<GuideStep>,
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
    let (ready, connected) = tracker_counts(&devices);
    let hub_stopped =
        hub == HubLive::Stopped || hub == HubLive::Unknown && snapshot.hub == HubState::Stopped;
    let guide = guide_steps(
        health != WatchHealth::NotFound && !hub_stopped,
        dongle,
        ready,
        connected,
    );

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
            let (headline, action) = fill_notice(&notice.text, target);
            FaultRow {
                headline,
                action,
                resolved: notice.step == Step::Resolved,
            }
        })
        .collect();

    let (overall, line, greyed) = match availability {
        Availability::Stopped | Availability::NotFound => {
            (Overall::Waiting, Some(Line::Guide(GuideStepId::G1)), false)
        }
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
                let line = Line::Fault {
                    headline: fault.headline.clone(),
                    action: fault.action.clone(),
                };
                (Overall::Fault, Some(line), false)
            } else if connected > 0 && ready == connected {
                (Overall::Ok, Some(Line::Guide(GuideStepId::G5)), false)
            } else {
                let line = guide
                    .iter()
                    .find(|step| !step.done)
                    .map(|step| Line::Guide(step.id));
                (Overall::Waiting, line, false)
            }
        }
    };

    DisplayModel {
        overall,
        line,
        faults,
        guide,
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

fn tracker_counts(devices: &[DeviceRow]) -> (usize, usize) {
    devices
        .iter()
        .filter(|row| row.connection != Connection::Disconnected)
        .fold((0, 0), |(ready, connected), row| {
            (
                ready + usize::from(row.state == Some(UserState::Ok)),
                connected + 1,
            )
        })
}

fn guide_steps(
    hub_ready: bool,
    dongle: DongleState,
    ready: usize,
    connected: usize,
) -> Vec<GuideStep> {
    use GuideStepId::{G1, G2, G3, G4, G5};
    let mut all_done = true;
    [
        (G1, hub_ready),
        (G2, dongle != DongleState::Disconnected),
        (G3, connected > 0),
        (G4, ready == connected),
        (G5, false),
    ]
    .into_iter()
    .map(|(id, done)| {
        all_done &= done;
        GuideStep {
            id,
            done: all_done,
            progress: (id == G4 && !all_done).then(|| {
                (
                    u16::try_from(ready).unwrap_or(u16::MAX),
                    u16::try_from(connected).unwrap_or(u16::MAX),
                )
            }),
        }
    })
    .collect()
}
