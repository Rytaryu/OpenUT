use openut_core::event::Battery;
use openut_core::state::{Connection, DongleState, HubState, State};
use openut_core::tracker::{HubNumber, Serial};
use openut_hublog::header::{FileKind, ParsedLine, parse_line};
use openut_hublog::map::{MapCounts, map_entry};

#[test]
fn maps_saved_tracker_properties_and_reconnection_into_the_display_snapshot() {
    let mut state = State::default();
    let mut counts = MapCounts::default();
    let mut apply = |state: &mut State, ms: u16, func: &str, body: &str| {
        let source = format!("[2026-11-28 21:00:00.{ms:03} +9 0xF88 INFO {func}] [3976] {body}");
        let ParsedLine::Entry(entry) = parse_line(&source) else {
            panic!("invalid synthetic record header");
        };
        let event = map_entry(&entry, FileKind::Server, &mut counts).expect("recognized event");
        state.apply(&event);
        event.at
    };
    apply(&mut state, 1, "main", "Tracker Server Starts.");
    apply(
        &mut state,
        2,
        "OETServer::AddTrackerCB",
        "Add new tracker: (2), connection type: 1",
    );
    apply(
        &mut state,
        3,
        "OETServer::GetDeviceInfoAndResponse",
        "Response - cmd type: 214, device index: 2, sn: (TRACKER-TEST-F50), model name: (VIVE Ultimate Tracker 1), fw version: (1.2.3)",
    );
    let at = apply(
        &mut state,
        4,
        "OETServer::DeviceStatusToVHServer",
        "display index: 2, device index: 2, connected: 1, tracking status: 7, charging: 1, percentage: 67, map ID: -1, map status: 6, map state: 2, is host: 1",
    );
    let snapshot = state.snapshot(at);
    assert_eq!(snapshot.hub, HubState::Running);
    assert_eq!(snapshot.dongle, DongleState::Connected);
    let [tracker] = snapshot.devices.as_slice() else {
        panic!("expected just one known tracker");
    };
    assert_eq!(tracker.serial, Some(Serial("TRACKER-TEST-F50".into())));
    assert_eq!(tracker.firmware.as_deref(), Some("1.2.3"));
    assert_eq!(tracker.hub_number, Some(HubNumber(1)));
    assert_eq!(tracker.connection, Connection::Dongle);
    assert_eq!(
        tracker.battery,
        Some(Battery {
            percent: 67,
            charging: Some(true),
        })
    );
    assert!(tracker.host);
    let old_connection = tracker.conn;

    let at = apply(
        &mut state,
        5,
        "OETServer::RemoveTrackerCB",
        "Remove tracker (2), connection type: 1, power state:11 ",
    );
    let snapshot = state.snapshot(at);
    let tracker = &snapshot.devices[0];
    assert_eq!(tracker.connection, Connection::Disconnected);
    assert_eq!(tracker.firmware.as_deref(), Some("1.2.3"));
    assert_eq!(tracker.hub_number, None);
    assert_eq!(tracker.battery, None);
    assert!(!tracker.host);

    apply(
        &mut state,
        6,
        "OETServer::AddTrackerCB",
        "Add new tracker: (4), connection type: 2",
    );
    let at = apply(
        &mut state,
        7,
        "OETServer::GetDeviceInfoAndResponse",
        "Response - cmd type: 214, device index: 4, sn: (TRACKER-TEST-F50), model name: (VIVE Ultimate Tracker 3)",
    );
    let snapshot = state.snapshot(at);
    let [tracker] = snapshot.devices.as_slice() else {
        panic!("expected the original tracker after reconnection");
    };
    assert_eq!(tracker.connection, Connection::Usb);
    assert_eq!(tracker.hub_number, Some(HubNumber(3)));
    assert_eq!(tracker.firmware.as_deref(), Some("1.2.3"));
    assert_ne!(tracker.conn, old_connection);
    assert_eq!(tracker.battery, None);
    assert!(!tracker.host);
    assert_eq!(counts.server.unparsed, 0);
    assert_eq!(snapshot.roster[0].last_seen, at);
}
