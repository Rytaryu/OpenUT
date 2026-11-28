//! Classify a Hub log entry by its body and read the values used by the watcher.

use crate::header::{Entry, FileKind};

/// A supported log record, including only the fields the watcher uses.
#[derive(Debug, PartialEq, Eq)]
pub enum Record<'a> {
    ServerStart,
    ServerRelease,
    DongleRemoved,
    TrackerAdded {
        tracker: u16,
        connection: i64,
    },
    TrackerRemoved {
        tracker: u16,
        connection: i64,
    },
    DeviceInfo {
        tracker: u16,
        serial: &'a str,
    },
    HostMode(u16),
    HostWifiQuery(u16),
    HostWifiGot,
    WifiAssign(u16),
    WifiNotYet,
    ClientLink {
        tracker: u16,
        connected: bool,
    },
    DeviceStatus {
        tracker: u16,
        connected: i64,
        host: i64,
        map_status: i64,
        map_state: i64,
        usb: Option<bool>,
    },
    Pose {
        tracker: u16,
        status: i64,
    },
    OetLost(u16),
    OetReloc(u16),
    OetOverspeed(u16),
    Command {
        kind: &'static str,
        tracker: Option<u16>,
    },
    PowerOffNotice(u16),
    /// No supported body shape matched.
    Other,
    /// A supported shape matched, but a required value could not be read.
    Unparsed,
}

/// Recognize the documented body prefixes for the input file (§4.4).
pub fn classify<'a>(entry: &Entry<'a>, file: FileKind) -> Record<'a> {
    let body = entry.body;
    if file == FileKind::Driver {
        return bracketed_tracker(
            body,
            '(',
            ')',
            " Tracker is going to power-off.",
            Record::PowerOffNotice,
        );
    }

    // Most input records do not describe a watcher event. The leading byte
    // limits the number of known prefixes checked for each record.
    match body.as_bytes().first() {
        Some(b'A') if let Some(tail) = body.strip_prefix("Add new tracker: (") => {
            added_tracker(tail).unwrap_or(Record::Unparsed)
        }
        Some(b'A')
            if let Some(tail) =
                body.strip_prefix("Assign wifi info to client tracker (index: ") =>
        {
            tracker_before(tail, ").")
                .map_or(Record::Unparsed, |(tracker, _)| Record::WifiAssign(tracker))
        }
        Some(b'C') if let Some(tail) = body.strip_prefix("Client tracker (index: ") => {
            client_link(tail).unwrap_or(Record::Unparsed)
        }
        Some(b'C') if body.starts_with("Cancel pairing on dongle.") => command("unpair", None),
        Some(b'C')
            if entry.func == "OETServer::SetCameraFrameRateToAllTrackers"
                && let Some(tail) = body.strip_prefix("Camera frame rate:") =>
        {
            command_with_value("camera_rate", tail)
        }
        Some(b'D') if body.starts_with("Dongle is removed.") => Record::DongleRemoved,
        Some(b'F') if let Some(tail) = body.strip_prefix("Fail to stop tracker, err:") => {
            command_with_value("stop", tail)
        }
        Some(b'F') if let Some(tail) = body.strip_prefix("FAILED TO POWER OFF. index: ") => {
            power_off(tracker_at_start(tail))
        }
        Some(b'G') if body.starts_with("Got host wifi info.") => Record::HostWifiGot,
        Some(b'O') if let Some(tail) = body.strip_prefix("OET LOST TRACKING! index: ") => {
            tracker_at_start(tail).map_or(Record::Unparsed, Record::OetLost)
        }
        Some(b'O') if let Some(tail) = body.strip_prefix("OET reloc back. index: ") => {
            tracker_at_start(tail).map_or(Record::Unparsed, Record::OetReloc)
        }
        Some(b'P') if let Some(tail) = body.strip_prefix("Pair tracker, isPair: ") => {
            pair_command(tail).unwrap_or(Record::Unparsed)
        }
        Some(b'P') if let Some(tail) = body.strip_prefix("PowerOff tracker (") => {
            power_off(read_power_off_tracker(tail))
        }
        Some(b'Q') if let Some(tail) = body.strip_prefix("Query wifi from tracker ") => {
            tracker_at_start(tail).map_or(Record::Unparsed, Record::HostWifiQuery)
        }
        Some(b'R') if let Some(tail) = body.strip_prefix("Remove tracker (") => {
            removed_tracker(tail).unwrap_or(Record::Unparsed)
        }
        Some(b'R') if body.starts_with("Release begin") => Record::ServerRelease,
        Some(b'R')
            if body.starts_with("Response - cmd type: ")
                && let Some((fields, serial)) = body.split_once(", sn: (") =>
        {
            device_info(fields, serial).unwrap_or(Record::Unparsed)
        }
        Some(b'S') => start_stop_or_standby(body),
        Some(b'T') if body.starts_with("Tracker Server Starts.") => Record::ServerStart,
        Some(b'T') if let Some(tail) = body.strip_prefix("Tracker (") => tracker_mode(tail),
        Some(b'U') if body.starts_with("Unpair All") => command("unpair", None),
        Some(b'W') if body.starts_with("Wifi info is not yet available.") => Record::WifiNotYet,
        Some(b'W') if body.starts_with("Wake up dongle.") => command("wake_dongle", None),
        Some(b'W') if let Some(tail) = body.strip_prefix("Wake up dongle: (") => {
            tracker_before(tail, ").").map_or(Record::Unparsed, |(tracker, _)| {
                command("wake_dongle", Some(tracker))
            })
        }
        Some(b'[') => bracketed_tracker(body, '[', ']', " OET OVERSPEED!", Record::OetOverspeed),
        Some(b'(') => classify_pose(body),
        Some(b'd') if body.starts_with("display index: ") => {
            device_status(body).unwrap_or(Record::Unparsed)
        }
        _ => Record::Other,
    }
}

fn added_tracker(tail: &str) -> Option<Record<'_>> {
    let (tracker, connection) = tracker_before(tail, "), connection type: ")?;
    Some(Record::TrackerAdded {
        tracker,
        connection: integer_at_start(connection)?,
    })
}

fn client_link(tail: &str) -> Option<Record<'_>> {
    let (tracker, connected) = tracker_before(tail, ") is connected to host: ")?;
    Some(Record::ClientLink {
        tracker,
        connected: read_switch(integer_at_start(connected)?)?,
    })
}

fn start_stop_or_standby(body: &str) -> Record<'_> {
    if body.starts_with("Stop tracker.") {
        return command("stop", None);
    }
    if body.starts_with("Set dongle into standby.") {
        return command("standby_dongle", None);
    }
    for prefix in [
        "Start tracker - power state :",
        "Start tracker. state:",
        "Start host tracker. ret:",
    ] {
        if let Some(tail) = body.strip_prefix(prefix) {
            return command_with_value("start", tail);
        }
    }
    Record::Other
}

fn tracker_mode(tail: &str) -> Record<'_> {
    let Some((tracker, suffix)) = tail.split_once(')') else {
        return Record::Other;
    };
    if suffix.starts_with(" - switch to HOST mode.") {
        return tracker.parse().map_or(Record::Unparsed, Record::HostMode);
    }
    if suffix.starts_with(" - switch to CLIENT mode.") {
        return tracker.parse().map_or(Record::Unparsed, |tracker| {
            command("client_mode", Some(tracker))
        });
    }
    Record::Other
}

fn classify_pose(body: &str) -> Record<'_> {
    let Some((indices, status)) = body
        .strip_prefix("(SteamVR: ")
        .and_then(|tail| tail.split_once("]Pose status is changed: Pose data - status: "))
    else {
        return Record::Other;
    };
    pose(indices, status).unwrap_or(Record::Unparsed)
}

fn pose<'a>(indices: &str, status: &str) -> Option<Record<'a>> {
    let (steamvr, tail) = indices.split_once(")(")?;
    let (tracker, counter) = tail.split_once(")[")?;
    steamvr.parse::<i64>().ok()?;
    counter.parse::<i64>().ok()?;
    Some(Record::Pose {
        tracker: tracker.parse().ok()?,
        status: status.split_once(',')?.0.parse().ok()?,
    })
}

fn bracketed_tracker<'a>(
    body: &str,
    opener: char,
    closer: char,
    suffix: &str,
    record: impl FnOnce(u16) -> Record<'a>,
) -> Record<'a> {
    let Some((tracker, tail)) = body
        .strip_prefix(opener)
        .and_then(|tail| tail.split_once(closer))
    else {
        return Record::Other;
    };
    if tail.starts_with(suffix) {
        tracker.parse().map_or(Record::Unparsed, record)
    } else {
        Record::Other
    }
}

fn device_info<'a>(fields: &str, serial: &'a str) -> Option<Record<'a>> {
    let (_, index) = fields.split_once(", device index: ")?;
    let (tracker, _) = tracker_before(index, ",")?;
    let (serial, _) = serial.split_once(')')?;
    if serial.trim_ascii().is_empty() {
        return None;
    }
    Some(Record::DeviceInfo { tracker, serial })
}

fn device_status(body: &str) -> Option<Record<'_>> {
    let mut tracker = None;
    let mut connected = None;
    let mut host = None;
    let mut map_status = None;
    let mut map_state = None;
    let mut usb = None;
    for (key, value) in body.split(", ").filter_map(|field| field.split_once(": ")) {
        let value = value.trim_ascii();
        match key {
            "device index" => tracker = value.parse().ok(),
            "connected" => connected = value.parse().ok(),
            "is host" => host = value.parse().ok(),
            "map status" => map_status = value.parse().ok(),
            "map state" => map_state = value.parse().ok(),
            "bIsConnectedOnUSB" => usb = value.parse().ok().and_then(read_switch),
            _ => {}
        }
    }
    Some(Record::DeviceStatus {
        tracker: tracker?,
        connected: connected?,
        host: host?,
        map_status: map_status?,
        map_state: map_state?,
        usb,
    })
}

fn removed_tracker(tail: &str) -> Option<Record<'_>> {
    let (tracker, tail) = tracker_before(tail, "), connection type: ")?;
    let (connection, power_state) = tail.split_once(", power state:")?;
    integer_at_start(power_state)?;
    Some(Record::TrackerRemoved {
        tracker,
        connection: connection.parse().ok()?,
    })
}

fn pair_command(tail: &str) -> Option<Record<'_>> {
    let (pair, index) = tail.split_once(", device index:")?;
    pair.parse::<i64>().ok()?;
    let index = integer_at_start(index)?;
    let tracker = if index < 0 {
        None
    } else {
        Some(u16::try_from(index).ok()?)
    };
    Some(command("pair", tracker))
}

fn read_power_off_tracker(tail: &str) -> Option<u16> {
    let (tracker, error) = tracker_before(tail, ") failed, err:")?;
    integer_at_start(error)?;
    Some(tracker)
}

fn power_off<'a>(tracker: Option<u16>) -> Record<'a> {
    tracker.map_or(Record::Unparsed, |tracker| {
        command("power_off", Some(tracker))
    })
}

fn tracker_before<'a>(tail: &'a str, separator: &str) -> Option<(u16, &'a str)> {
    let (tracker, tail) = tail.split_once(separator)?;
    Some((tracker.parse().ok()?, tail))
}

fn command<'a>(kind: &'static str, tracker: Option<u16>) -> Record<'a> {
    Record::Command { kind, tracker }
}

fn command_with_value<'a>(kind: &'static str, tail: &str) -> Record<'a> {
    integer_at_start(tail).map_or(Record::Unparsed, |_| command(kind, None))
}

fn read_switch(value: i64) -> Option<bool> {
    match value {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn integer_at_start(tail: &str) -> Option<i64> {
    tail.trim_start_matches([' ', '\t'])
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .next()?
        .parse()
        .ok()
}

fn tracker_at_start(tail: &str) -> Option<u16> {
    u16::try_from(integer_at_start(tail)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{ParsedLine, parse_line};

    // The three DeviceStatus records in hub-log-samples.md.
    const STATUS_SAMPLES: [&str; 3] = [
        "[2026-11-28 00:00:53.990 +9 0x3410 INFO OETServer::DeviceStatusToVHServer] [13328] display index: 0, device index: 2, connected: 0, tracking status: 0, charging: 0, percentage: 0, map ID: 0, map status: 0, has map: 0, map state: 0, is host: 0, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 0, indicator value: 0",
        "[2026-11-27 00:01:03.045 +9 0x3240 INFO OETServer::DeviceStatusToVHServer] [12864] display index: 2, device index: 4, connected: 1, tracking status: 2, charging: 0, percentage: 74, map ID: 0, map status: 6, has map: 1, map state: 2, is host: 0, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 0, indicator value: 0",
        "[2026-11-27 22:53:25.380 +9 0xF88 INFO OETServer::DeviceStatusToVHServer] [3976] display index: 0, device index: 2, connected: 1, tracking status: 5, charging: 0, percentage: 98, map ID: -1, map status: -1, has map: 1, map state: 0, is host: 1, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 1, indicator value: 1",
    ];

    fn classify_line(line: &str, file: FileKind) -> Record<'_> {
        let ParsedLine::Entry(entry) = parse_line(line) else {
            panic!("expected a log entry");
        };
        classify(&entry, file)
    }

    fn assert_server_body(body: &str, expected: Record<'_>) {
        let line = format!("[2026-11-27 22:53:25.380 +9 0x1 INFO f] [1] {body}");
        assert_eq!(classify_line(&line, FileKind::Server), expected, "{body}");
    }

    #[test]
    fn reads_the_three_documented_device_status_records() {
        for (line, tracker, connected, host, map_status, map_state) in [
            (STATUS_SAMPLES[0], 2, 0, 0, 0, 0),
            (STATUS_SAMPLES[1], 4, 1, 0, 6, 2),
            (STATUS_SAMPLES[2], 2, 1, 1, -1, 0),
        ] {
            assert_eq!(
                classify_line(line, FileKind::Server),
                Record::DeviceStatus {
                    tracker,
                    connected,
                    host,
                    map_status,
                    map_state,
                    usb: Some(false)
                }
            );
        }
    }

    #[test]
    fn accepts_missing_or_invalid_usb_without_discarding_device_status() {
        for (field, usb) in [
            (", bIsConnectedOnUSB: 1", Some(true)),
            (", bIsConnectedOnUSB: 2", None),
            (", bIsConnectedOnUSB: unavailable", None),
            ("", None),
        ] {
            let line = STATUS_SAMPLES[2].replace(", bIsConnectedOnUSB: 0", field);
            assert_eq!(
                classify_line(&line, FileKind::Server),
                Record::DeviceStatus {
                    tracker: 2,
                    connected: 1,
                    host: 1,
                    map_status: -1,
                    map_state: 0,
                    usb
                }
            );
        }
    }

    #[test]
    fn requires_each_device_status_field_used_by_the_watcher() {
        for field in [
            ", device index: 2",
            ", connected: 1",
            ", map status: -1",
            ", map state: 0",
            ", is host: 1",
        ] {
            let missing = STATUS_SAMPLES[2].replace(field, "");
            let (key, _) = field.split_once(": ").expect("a key and value");
            let invalid = STATUS_SAMPLES[2].replace(field, &format!("{key}: invalid"));
            for line in [&missing, &invalid] {
                assert_eq!(
                    classify_line(line, FileKind::Server),
                    Record::Unparsed,
                    "{line}"
                );
            }
        }
    }

    #[test]
    fn distinguishes_malformed_records_from_unrecognized_shapes() {
        for body in [
            "Remove tracker (x), connection type: 1, power state:11 ",
            "Add new tracker: (65536), connection type: 2",
            "Assign wifi info to client tracker (index: -2).",
            "Client tracker (index: 4) is connected to host: 2",
            "Pair tracker, isPair: 0, device index:999999999999999999999",
            "Response - cmd type: 214, device index: 2, result: 0, sn: (), role: (0)",
            "Response - cmd type: 214, device index: 2, result: 0, sn: (unterminated",
            "(SteamVR: -1)(2)[44]Pose status is changed: Pose data - status: ",
            "(SteamVR: -1)(x)[44]Pose status is changed: Pose data - status: 2, pos: (-nan, -nan, -nan)",
            "[x] OET OVERSPEED!",
        ] {
            assert_server_body(body, Record::Unparsed);
        }
        for body in [
            "Tracker (2) Switching mode: ON",
            "(SteamVR: -1)(0)(tracking status: 0) Position (-nan, -nan, -nan) includes NAN.",
            "Response - cmd type: 214, device index: 2, result: 0",
            "This OET has latest firmware",
            "Tracker is not found",
            "[ServiceStatusToVHServer] unrecognized [2] OET OVERSPEED!",
            "(2) Tracker is going to power-off.",
        ] {
            assert_server_body(body, Record::Other);
        }
    }

    #[test]
    fn driver_only_classifies_power_off_notices() {
        let header =
            "[2026-11-27 22:52:59.298 +9 0xF88 INFO TrackerDongleHID::ParseIndication] [3976]";
        for (body, expected) in [
            (
                "(2) Tracker is going to power-off.",
                Record::PowerOffNotice(2),
            ),
            ("(65536) Tracker is going to power-off.", Record::Unparsed),
            ("(2) set power state: 21", Record::Other),
            ("Wait for HID data got timeout.\n", Record::Other),
            ("OET LOST TRACKING! index: 2", Record::Other),
        ] {
            let line = format!("{header} {body}");
            assert_eq!(classify_line(&line, FileKind::Driver), expected);
        }
    }

    #[test]
    fn arbitrary_bodies_do_not_panic() {
        const PREFIXES: [&str; 6] = [
            "",
            "Remove tracker (",
            "display index: ",
            "[",
            "(",
            "(SteamVR: ",
        ];
        const CHARS: [char; 11] = ['0', '9', '-', ',', '[', ']', ')', '(', ' ', '\n', '時'];
        let mut seed = 0xe37b_94c1_u32;
        for case in 0..10_000 {
            let mut body = PREFIXES[case % PREFIXES.len()].to_owned();
            for _ in 0..case % 41 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                body.push(CHARS[(seed >> 16) as usize % CHARS.len()]);
            }
            let line = format!("[2026-11-27 22:53:25.380 +9 0x1 INFO f] [1] {body}");
            for file in [FileKind::Server, FileKind::Driver] {
                let _ = classify_line(&line, file);
            }
        }
    }
}
