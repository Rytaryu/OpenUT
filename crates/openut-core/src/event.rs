//! Source-independent input to tracker state, detection rules, and summaries.

use crate::time::{LogTime, UtcOffset};
use crate::tracker::{HubNumber, Serial, TrackerKey};

/// An event and the UTC instant and local offset at which it occurred.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timed {
    pub at: LogTime,
    pub offset: UtcOffset,
    pub event: Event,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    ServerStarted,
    ServerStopped,
    DongleAdded,
    /// Disconnects the dongle's trackers. Direct USB connections are separate.
    DongleRemoved,
    /// Reports the dongle's state without changing tracker connections.
    DongleReport {
        connected: bool,
    },
    TrackerAdded {
        tracker: TrackerKey,
        usb: bool,
    },
    TrackerRemoved {
        tracker: TrackerKey,
        usb: bool,
    },
    SerialKnown {
        tracker: TrackerKey,
        serial: Serial,
        hub_number: Option<HubNumber>,
        firmware: Option<String>,
    },
    HostAssigned {
        tracker: TrackerKey,
    },
    HostWifiQueried {
        host: TrackerKey,
    },
    HostWifiObtained {
        host: Option<TrackerKey>,
    },
    WifiRequestUnanswered {
        tracker: Option<TrackerKey>,
    },
    WifiAssigned {
        tracker: TrackerKey,
    },
    ClientLink {
        tracker: TrackerKey,
        connected: bool,
    },
    /// A report of the current state. The source may omit the connection path.
    Status {
        tracker: TrackerKey,
        connected: bool,
        host: bool,
        map_ready: bool,
        usb: Option<bool>,
        battery: Option<Battery>,
    },
    Pose {
        tracker: TrackerKey,
        status: PoseStatus,
    },
    Oet {
        tracker: TrackerKey,
        kind: OetKind,
    },
    PowerOffNotice {
        tracker: TrackerKey,
    },
    /// A PC command. An absent tracker denotes a command for all trackers.
    PcCommand {
        tracker: Option<TrackerKey>,
        kind: String,
    },
}

/// The last reported percentage and charging state of a connected tracker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charging: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoseStatus {
    Ok,
    Lost,
    RecentlyLost,
    Other(i64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OetKind {
    LostTracking,
    RelocBack,
    Overspeed,
}
