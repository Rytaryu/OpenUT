//! Map classified Hub text records to source-independent events (watcher design §4.4).

use openut_core::event::{Event, OetKind, PoseStatus, Timed};
use openut_core::tracker::{HubNumber, Serial, TrackerKey};

use crate::header::{Entry, FileKind};
use crate::record::{Record, classify};

/// Counts for entries with readable headers, kept separately for each file stream.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct MapCounts {
    pub server: RecordCounts,
    pub driver: RecordCounts,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct RecordCounts {
    pub lines: usize,
    pub other: usize,
    pub unparsed: usize,
}

/// Classify a record and convert it into an event, preserving the given timestamp.
/// Pass the entry through `OffsetFill` first when correlating server and driver logs.
pub fn map_entry(entry: &Entry<'_>, file: FileKind, counts: &mut MapCounts) -> Option<Timed> {
    let stats = match file {
        FileKind::Server => &mut counts.server,
        FileKind::Driver => &mut counts.driver,
    };
    stats.lines = stats.lines.saturating_add(1);

    let event = match classify(entry, file) {
        Record::ServerStart => Event::ServerStarted,
        Record::ServerRelease => Event::ServerStopped,
        Record::DongleAdded => Event::DongleAdded,
        Record::DongleRemoved => Event::DongleRemoved,
        Record::ServiceStatus { connected } => Event::DongleReport { connected },
        Record::TrackerAdded {
            tracker,
            connection,
        } => Event::TrackerAdded {
            tracker: TrackerKey(tracker),
            usb: connection == 2,
        },
        Record::TrackerRemoved {
            tracker,
            connection,
        } => Event::TrackerRemoved {
            tracker: TrackerKey(tracker),
            usb: connection == 2,
        },
        Record::DeviceInfo {
            tracker,
            serial,
            hub_number,
            firmware,
        } => Event::SerialKnown {
            tracker: TrackerKey(tracker),
            serial: Serial(serial.to_owned()),
            hub_number: hub_number.map(HubNumber),
            firmware: firmware.map(str::to_owned),
        },
        Record::HostMode(tracker) => Event::HostAssigned {
            tracker: TrackerKey(tracker),
        },
        Record::HostWifiQuery(host) => Event::HostWifiQueried {
            host: TrackerKey(host),
        },
        Record::HostWifiGot => Event::HostWifiObtained { host: None },
        Record::WifiAssign(tracker) => Event::WifiAssigned {
            tracker: TrackerKey(tracker),
        },
        Record::WifiNotYet => Event::WifiRequestUnanswered { tracker: None },
        Record::ClientLink { tracker, connected } => Event::ClientLink {
            tracker: TrackerKey(tracker),
            connected,
        },
        Record::DeviceStatus {
            tracker,
            connected,
            host,
            map_status,
            map_state,
            usb,
            battery,
        } => Event::Status {
            tracker: TrackerKey(tracker),
            connected: connected == 1,
            host: host == 1,
            map_ready: map_state == 2 || map_status == 6,
            usb,
            battery,
        },
        Record::Pose { tracker, status } => Event::Pose {
            tracker: TrackerKey(tracker),
            status: match status {
                2 => PoseStatus::Ok,
                3 => PoseStatus::Lost,
                4 => PoseStatus::RecentlyLost,
                status => PoseStatus::Other(status),
            },
        },
        Record::OetLost(tracker) => Event::Oet {
            tracker: TrackerKey(tracker),
            kind: OetKind::LostTracking,
        },
        Record::OetReloc(tracker) => Event::Oet {
            tracker: TrackerKey(tracker),
            kind: OetKind::RelocBack,
        },
        Record::OetOverspeed(tracker) => Event::Oet {
            tracker: TrackerKey(tracker),
            kind: OetKind::Overspeed,
        },
        Record::Command { kind, tracker } => Event::PcCommand {
            tracker: tracker.map(TrackerKey),
            kind: kind.to_owned(),
        },
        Record::PowerOffNotice(tracker) => Event::PowerOffNotice {
            tracker: TrackerKey(tracker),
        },
        Record::Other => {
            // The driver stream only contributes shutdown notices.
            stats.other = stats
                .other
                .saturating_add(usize::from(file == FileKind::Server));
            return None;
        }
        Record::Unparsed => {
            stats.unparsed = stats.unparsed.saturating_add(1);
            return None;
        }
    };
    Some(Timed {
        at: entry.at,
        offset: entry.offset,
        event,
    })
}

#[cfg(test)]
mod tests {
    use openut_core::time::{LogTime, UtcOffset};

    use super::*;

    fn sample(body: &str, at: i64) -> Entry<'_> {
        Entry {
            at: LogTime(at),
            offset: UtcOffset(-330),
            offset_known: true,
            level: "WARN",
            func: "SyntheticLog",
            thread: Some("12"),
            body,
        }
    }

    #[test]
    fn repeated_bodies_keep_their_individual_times_and_offsets() {
        let mut counts = MapCounts::default();
        for at in [32_000, 32_003] {
            let entry = sample("Wifi info is not yet available.", at);
            assert_eq!(
                map_entry(&entry, FileKind::Server, &mut counts),
                Some(Timed {
                    at: LogTime(at),
                    offset: UtcOffset(-330),
                    event: Event::WifiRequestUnanswered { tracker: None },
                })
            );
        }
        assert_eq!(counts.server.lines, 2);
    }

    #[test]
    fn counts_each_stream_and_maps_only_relevant_records() {
        let mut counts = MapCounts::default();
        for (file, body) in [
            (FileKind::Server, "(2) Tracker is going to power-off."),
            (
                FileKind::Server,
                "Remove tracker (x), connection type: 1, power state:11 ",
            ),
            (FileKind::Driver, "Wait for HID data got timeout.\n"),
            (FileKind::Driver, "Tracker Server Starts."),
            (FileKind::Driver, "(x) Tracker is going to power-off."),
        ] {
            assert_eq!(map_entry(&sample(body, 0), file, &mut counts), None);
        }
        assert!(matches!(
            map_entry(
                &sample("(2) Tracker is going to power-off.", 0),
                FileKind::Driver,
                &mut counts,
            ),
            Some(Timed {
                event: Event::PowerOffNotice {
                    tracker: TrackerKey(2)
                },
                ..
            })
        ));
        assert_eq!(
            counts,
            MapCounts {
                server: RecordCounts {
                    lines: 2,
                    other: 1,
                    unparsed: 1,
                },
                driver: RecordCounts {
                    lines: 4,
                    other: 0,
                    unparsed: 1,
                },
            }
        );
    }
}
