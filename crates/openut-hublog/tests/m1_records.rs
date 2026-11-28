use openut_core::event::{Battery, Event, OetKind, PoseStatus};
use openut_core::time::UtcOffset;
use openut_core::tracker::{HubNumber, Serial, TrackerKey};
use openut_hublog::decode::Decoder;
use openut_hublog::header::{Entry, FileKind, OffsetFill, ParsedLine, parse_line};
use openut_hublog::map::{MapCounts, map_entry};
use openut_hublog::record::{Record, classify};

const CAMERA_FRAME_RATE: &str = "Camera frame rate: 50";

fn read_entry(line: &str) -> Entry<'_> {
    let ParsedLine::Entry(entry) = parse_line(line) else {
        panic!("expected a readable test header: {line}");
    };
    entry
}

fn server_line(function: &str, level: &str, body: &str) -> String {
    format!("[2026-11-27 22:53:34.900 +9 0xF88 {level} {function}] [3976] {body}")
}

#[test]
fn server_sample_shapes_classify_and_map_to_events() {
    let tracker = TrackerKey(2);
    let cases = [
        (
            "OETServer::OnDongleAddedCB",
            "INFO",
            "Dongle is added. version: (0909.0000.0000.0917)",
            Record::DongleAdded,
            Event::DongleAdded,
        ),
        (
            "main",
            "INFO",
            "Tracker Server Starts.",
            Record::ServerStart,
            Event::ServerStarted,
        ),
        (
            "OETServer::Release",
            "INFO",
            "Release begin",
            Record::ServerRelease,
            Event::ServerStopped,
        ),
        (
            "OETServer::OnDongleRemovedCB",
            "INFO",
            "Dongle is removed.",
            Record::DongleRemoved,
            Event::DongleRemoved,
        ),
        (
            "OETServer::AddTrackerCB",
            "INFO",
            "Add new tracker: (2), connection type: 1",
            Record::TrackerAdded {
                tracker: 2,
                connection: 1,
            },
            Event::TrackerAdded {
                tracker,
                usb: false,
            },
        ),
        (
            "OETServer::RemoveTrackerCB",
            "INFO",
            "Remove tracker (2), connection type: 1, power state:11 ",
            Record::TrackerRemoved {
                tracker: 2,
                connection: 1,
            },
            Event::TrackerRemoved {
                tracker,
                usb: false,
            },
        ),
        (
            "OETServer::RemoveTrackerCB",
            "INFO",
            "Remove tracker (2), connection type: 2, power state:21 ",
            Record::TrackerRemoved {
                tracker: 2,
                connection: 2,
            },
            Event::TrackerRemoved { tracker, usb: true },
        ),
        (
            "OETServer::GetDeviceInfoAndResponse",
            "INFO",
            "Response - cmd type: 214, cmd key: (AAAA), device index: 2, result: 0, sn: (00XX000000001), model name: (VIVE Ultimate Tracker 1), fw version: (1.0.999.178), role: (0), device type: 0, force update: 0, type: 1",
            Record::DeviceInfo {
                tracker: 2,
                serial: "00XX000000001",
                hub_number: Some(1),
                firmware: Some("1.0.999.178"),
            },
            Event::SerialKnown {
                tracker,
                serial: Serial("00XX000000001".into()),
                hub_number: Some(HubNumber(1)),
                firmware: Some("1.0.999.178".into()),
            },
        ),
        (
            "Tracker::UpdateSwitchingMode",
            "INFO",
            "Tracker (2) - switch to HOST mode.",
            Record::HostMode(2),
            Event::HostAssigned { tracker },
        ),
        (
            "OETServer::TrackerSwitchAndResponse",
            "INFO",
            "Query wifi from tracker 2",
            Record::HostWifiQuery(2),
            Event::HostWifiQueried { host: tracker },
        ),
        (
            "OETServer::ProcessDeviceMsg",
            "INFO",
            "Got host wifi info.",
            Record::HostWifiGot,
            Event::HostWifiObtained { host: None },
        ),
        (
            "OETServer::ProcessDeviceMsg",
            "INFO",
            "Assign wifi info to client tracker (index: 2).",
            Record::WifiAssign(2),
            Event::WifiAssigned { tracker },
        ),
        (
            "OETServer::ProcessDeviceMsg",
            "WARN",
            "Wifi info is not yet available.",
            Record::WifiNotYet,
            Event::WifiRequestUnanswered { tracker: None },
        ),
        (
            "OETServer::ProcessDeviceMsg",
            "INFO",
            "Client tracker (index: 2) is connected to host: 0",
            Record::ClientLink {
                tracker: 2,
                connected: false,
            },
            Event::ClientLink {
                tracker,
                connected: false,
            },
        ),
        (
            "OETServer::ProcessDeviceMsg",
            "INFO",
            "Client tracker (index: 2) is connected to host: 1",
            Record::ClientLink {
                tracker: 2,
                connected: true,
            },
            Event::ClientLink {
                tracker,
                connected: true,
            },
        ),
        (
            "Tracker::CopyFromDriverThreadProc",
            "INFO",
            "(SteamVR: 5)(2)[49]Pose status is changed: Pose data - status: 3, pos: (0.000000, 0.000000, 0.000000), rotation: (0.123456, -0.123456, -0.123456, w 0.123456), vel: (0.000000, 0.000000, -0.000000), angular vel: (0.123456, -0.123456, -0.123456) ",
            Record::Pose {
                tracker: 2,
                status: 3,
            },
            Event::Pose {
                tracker,
                status: PoseStatus::Lost,
            },
        ),
        (
            "OETServer::OnTrackerErrorNotification",
            "ERROR",
            "OET LOST TRACKING! index: 2",
            Record::OetLost(2),
            Event::Oet {
                tracker,
                kind: OetKind::LostTracking,
            },
        ),
        (
            "OETServer::OnTrackerErrorNotification",
            "INFO",
            "OET reloc back. index: 2",
            Record::OetReloc(2),
            Event::Oet {
                tracker,
                kind: OetKind::RelocBack,
            },
        ),
        (
            "OETServer::OnTrackerStatusIndicatorUpdate",
            "ERROR",
            "[2] OET OVERSPEED!",
            Record::OetOverspeed(2),
            Event::Oet {
                tracker,
                kind: OetKind::Overspeed,
            },
        ),
    ];
    let mut counts = MapCounts::default();
    let total = cases.len();
    for (function, level, body, record, event) in cases {
        let line = server_line(function, level, body);
        let entry = read_entry(&line);
        assert_eq!(classify(&entry, FileKind::Server), record, "{body}");
        let timed = map_entry(&entry, FileKind::Server, &mut counts).expect("expected event");
        assert_eq!(timed.event, event, "{body}");
        assert_eq!(timed.at, entry.at);
        assert_eq!(timed.offset, entry.offset);
    }
    assert_eq!(counts.server.lines, total);
    assert_eq!(counts.server.other, 0);
    assert_eq!(counts.server.unparsed, 0);
}

#[test]
fn command_sample_shapes_preserve_the_target_and_command_kind() {
    let cases = [
        (
            "client_mode",
            Some(2),
            "Tracker (2) - switch to CLIENT mode.",
        ),
        (
            "start",
            None,
            "Start tracker - power state :1, data format: 3",
        ),
        ("start", None, "Start tracker. state: 1, type: 2, ret: 0"),
        ("start", None, "Start host tracker. ret: 0"),
        ("stop", None, "Stop tracker."),
        ("stop", None, "Fail to stop tracker, err: -1"),
        ("pair", Some(0), "Pair tracker, isPair: 0, device index:0"),
        ("pair", None, "Pair tracker, isPair: 0, device index:-2"),
        ("unpair", None, "Unpair All"),
        ("unpair", None, "Cancel pairing on dongle."),
        ("wake_dongle", Some(2), "Wake up dongle: (2)."),
        ("wake_dongle", None, "Wake up dongle."),
        ("standby_dongle", None, "Set dongle into standby."),
        ("power_off", Some(2), "PowerOff tracker (2) failed, err: -1"),
        ("power_off", Some(2), "FAILED TO POWER OFF. index: 2"),
        ("camera_rate", None, CAMERA_FRAME_RATE),
    ];
    let mut counts = MapCounts::default();
    for (kind, tracker, body) in cases {
        let function = if kind == "camera_rate" {
            "OETServer::SetCameraFrameRateToAllTrackers"
        } else {
            "Synthetic"
        };
        let line = server_line(function, "INFO", body);
        let entry = read_entry(&line);
        assert_eq!(
            classify(&entry, FileKind::Server),
            Record::Command { kind, tracker },
            "{body}",
        );
        assert_eq!(
            map_entry(&entry, FileKind::Server, &mut counts).map(|timed| timed.event),
            Some(Event::PcCommand {
                tracker: tracker.map(TrackerKey),
                kind: kind.into(),
            }),
            "{body}",
        );
    }
    assert_eq!(counts.server.unparsed + counts.server.other, 0);
}

#[test]
fn camera_frame_rate_from_config_is_other_and_does_not_map_to_a_command() {
    let line = server_line("OETServer::ParseConfig", "INFO", CAMERA_FRAME_RATE);
    let entry = read_entry(&line);
    assert_eq!(classify(&entry, FileKind::Server), Record::Other);
    assert_eq!(
        map_entry(&entry, FileKind::Server, &mut MapCounts::default()),
        None,
    );
}

#[test]
fn translates_status_and_pose_values_into_domain_events() {
    let tracker = TrackerKey(4);
    let mut counts = MapCounts::default();
    for (map_status, map_state, usb_field, map_ready, usb) in [
        (-1, 0, ", bIsConnectedOnUSB: 0", false, Some(false)),
        (6, 0, ", bIsConnectedOnUSB: 1", true, Some(true)),
        (-1, 2, "", true, None),
    ] {
        let body = format!(
            "display index: 2, device index: 4, connected: 1, tracking status: 7, \
             charging: 0, map ID: -1, map status: {map_status}, map state: {map_state}, \
             is host: 1{usb_field}"
        );
        let line = server_line("OETServer::DeviceStatusToVHServer", "INFO", &body);
        assert_eq!(
            map_entry(&read_entry(&line), FileKind::Server, &mut counts).map(|row| row.event),
            Some(Event::Status {
                tracker,
                connected: true,
                host: true,
                map_ready,
                usb,
                battery: None,
            }),
        );
    }
    for (value, status) in [
        (2, PoseStatus::Ok),
        (3, PoseStatus::Lost),
        (4, PoseStatus::RecentlyLost),
        (0, PoseStatus::Other(0)),
        (47, PoseStatus::Other(47)),
    ] {
        let body = format!(
            "(SteamVR: -1)(4)[49]Pose status is changed: Pose data - status: {value}, pos: (-nan, -nan, -nan)"
        );
        let line = server_line("Tracker::CopyFromDriverThreadProc", "INFO", &body);
        assert_eq!(
            map_entry(&read_entry(&line), FileKind::Server, &mut counts).map(|row| row.event),
            Some(Event::Pose { tracker, status }),
        );
    }
    assert_eq!(counts.server.unparsed + counts.server.other, 0);
}

#[test]
fn optional_tracker_details_do_not_make_a_serial_report_unreadable() {
    const RESPONSE: &str = "Response - cmd type: 214, device index: 2, result: 0";
    let mut counts = MapCounts::default();
    for (model, firmware, number, version) in [
        ("", "", None, None),
        (
            ", model name: (VIVE Ultimate Tracker)",
            ", fw version: ()",
            None,
            None,
        ),
        (
            ", model name: (Other Device 1)",
            ", fw version: (1 two)",
            None,
            None,
        ),
        (
            ", model name: (VIVE Ultimate Tracker 0)",
            ", fw version: (111111111111111111111111111111111)",
            None,
            None,
        ),
        (", model name: (VIVE Ultimate Tracker 100)", "", None, None),
        (
            ", model name: (VIVE Ultimate Tracker 99)",
            ", fw version: (v1.2-rc_1)",
            Some(HubNumber(99)),
            Some("v1.2-rc_1"),
        ),
    ] {
        let body = format!("{RESPONSE}, sn: (TRACKER-A){model}{firmware}");
        let line = server_line("OETServer::GetDeviceInfoAndResponse", "INFO", &body);
        assert_eq!(
            map_entry(&read_entry(&line), FileKind::Server, &mut counts).map(|row| row.event),
            Some(Event::SerialKnown {
                tracker: TrackerKey(2),
                serial: Serial("TRACKER-A".into()),
                hub_number: number,
                firmware: version.map(str::to_owned),
            })
        );
    }
    assert_eq!(counts.server.unparsed, 0);

    for serial in [
        "",
        "FAKE 0001",
        "FAKE\u{001f}0001",
        "ＦＡＫＥ",
        "012345678901234567890123456789012",
    ] {
        let body = format!("{RESPONSE}, sn: ({serial})");
        let line = server_line("OETServer::GetDeviceInfoAndResponse", "INFO", &body);
        assert!(map_entry(&read_entry(&line), FileKind::Server, &mut counts).is_none());
    }
    assert_eq!(counts.server.unparsed, 5);
    assert_eq!(counts.server.other, 0);
}

#[test]
fn battery_requires_connection_and_a_valid_percentage() {
    let mut counts = MapCounts::default();
    for (connected, values, battery) in [
        (0, ", charging: 0, percentage: 0", None),
        (0, ", charging: 1, percentage: 98", None),
        (1, ", percentage: 0", None),
        (1, ", percentage: 101", None),
        (1, ", percentage: -1", None),
        (1, ", percentage: x", None),
        (1, "", None),
        (
            1,
            ", percentage: 1, charging: 2",
            Some(Battery {
                percent: 1,
                charging: None,
            }),
        ),
        (
            1,
            ", percentage: 100",
            Some(Battery {
                percent: 100,
                charging: None,
            }),
        ),
        (
            1,
            ", charging: 0, percentage: 98",
            Some(Battery {
                percent: 98,
                charging: Some(false),
            }),
        ),
        (
            1,
            ", charging: 1, percentage: 43",
            Some(Battery {
                percent: 43,
                charging: Some(true),
            }),
        ),
    ] {
        let body = format!(
            "display index: 0, device index: 2, connected: {connected}, map status: 6, map state: 0, is host: 1{values}"
        );
        let line = server_line("OETServer::DeviceStatusToVHServer", "INFO", &body);
        assert_eq!(
            map_entry(&read_entry(&line), FileKind::Server, &mut counts).map(|row| row.event),
            Some(Event::Status {
                tracker: TrackerKey(2),
                connected: connected == 1,
                host: true,
                map_ready: true,
                usb: None,
                battery,
            })
        );
    }
    assert_eq!(counts.server.unparsed + counts.server.other, 0);
}

#[test]
fn maps_dongle_status_and_ignores_battery_reports_without_a_tracker() {
    let mut counts = MapCounts::default();
    for (body, event) in [
        (
            "[ServiceStatusToVHServer] dongle connected: 1, tracker connection type: 2",
            Some(Event::DongleReport { connected: true }),
        ),
        (
            "[ServiceStatusToVHServer] tracker connection type: 2, dongle connected: 0",
            Some(Event::DongleReport { connected: false }),
        ),
        ("[ServiceStatusToVHServer] dongle connected: 2", None),
        ("[ServiceStatusToVHServer] tracker connection type: 2", None),
        ("dongle count: 0, tracker count: 0", None),
        (
            "Peripheral data changed - is charging: 0, battery percent: 74, type: 0",
            None,
        ),
    ] {
        let line = server_line("Synthetic", "INFO", body);
        assert_eq!(
            map_entry(&read_entry(&line), FileKind::Server, &mut counts).map(|row| row.event),
            event
        );
    }
    assert_eq!(counts.server.unparsed, 2);
    assert_eq!(counts.server.other, 2);
}

#[test]
fn decoded_driver_notice_matches_server_removal_after_offset_fill() {
    let driver = concat!(
        "# Log rotate by date at 2026-11-27 00:01:03.40\r\n",
        "[2026-11-27 22:52:59.000 0x3410 INFO TrackerDongleHID::DataDispatchThreadProc] [13328] ",
        "Wait for HID data got timeout.\n\r\n",
        "[2026-11-27 22:52:59.298 0xF88 INFO TrackerDongleHID::ParseIndication] [3976] ",
        "(2) Tracker is going to power-off.\r\n",
    );
    let server = concat!(
        "# Log rotate by date at 2026-11-27 00:01:03.40\r\n",
        "[2026-11-27 22:52:59.298 +9 0xF88 INFO OETServer::RemoveTrackerCB] [3976] ",
        "Remove tracker (2), connection type: 1, power state:11 \r\n",
    );
    let mut fill = OffsetFill::new();
    let mut counts = MapCounts::default();
    let mut events = Vec::new();
    for (file, text) in [(FileKind::Driver, driver), (FileKind::Server, server)] {
        let bytes: Vec<u8> = [0xfeff]
            .into_iter()
            .chain(text.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut decoder = Decoder::new();
        let mut records: Vec<_> = bytes
            .chunks(7)
            .flat_map(|chunk| decoder.push(chunk))
            .collect();
        records.extend(decoder.finish());
        for line in records {
            match parse_line(&line) {
                ParsedLine::Entry(entry) => fill.push(file, entry, |filled| {
                    if let Some(event) = map_entry(&filled.entry, filled.file, &mut counts) {
                        events.push(event);
                    }
                }),
                ParsedLine::Meta(_) => {}
                ParsedLine::NoHeader(_) => panic!("unexpected record without a header"),
            }
        }
    }
    let [notice, removal] = events.as_slice() else {
        panic!("expected only the driver notice and server removal");
    };
    let tracker = TrackerKey(2);
    assert_eq!(notice.event, Event::PowerOffNotice { tracker });
    assert_eq!(
        removal.event,
        Event::TrackerRemoved {
            tracker,
            usb: false
        }
    );
    assert_eq!(notice.at, removal.at);
    assert_eq!(notice.at.to_string(), "2026-11-27 13:52:59.298");
    assert_eq!(notice.offset, UtcOffset(540));
    assert_eq!(counts.driver.lines, 2);
    assert_eq!(counts.driver.other + counts.driver.unparsed, 0);
    assert_eq!(counts.server.lines, 1);
    assert_eq!(fill.bad_offset(FileKind::Driver), 2);
}
