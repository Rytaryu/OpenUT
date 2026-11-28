//! Source-independent input to tracker state, detection rules, and summaries.

use crate::time::{LogTime, UtcOffset};
use crate::tracker::{Serial, TrackerKey};

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
    /// Disconnects the dongle's trackers. Direct USB connections are separate.
    DongleRemoved,
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
