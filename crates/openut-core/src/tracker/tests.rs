use crate::event::{Event, Timed};
use crate::rules::Step;
use crate::state::{Phase, State};
use crate::time::{LogTime, UtcOffset};

use super::*;

const A: &str = "SYNTHETICA0001";
const B: &str = "SYNTHETICA0002";
const C: &str = "SYNTHETICA0003";

fn roster(serials: &[&str]) -> Vec<Identity> {
    serials
        .iter()
        .map(|serial| Identity {
            serial: Serial((*serial).into()),
            firmware: None,
            last_seen: LogTime(0),
        })
        .collect()
}

fn device(serial: Option<&str>, number: Option<u16>, connection: Connection) -> DeviceView {
    DeviceView {
        serial: serial.map(|serial| Serial(serial.into())),
        firmware: None,
        hub_number: number.map(HubNumber),
        key: Some(TrackerKey(9)),
        conn: Some(ConnId(1)),
        host: false,
        connection,
        battery: None,
        phase: Phase::Connected,
    }
}

#[test]
fn same_shortest_suffix_width_is_used_for_the_whole_roster() {
    for serials in [
        [A, B, C].as_slice(),
        [A, B, C, "SYNTHETICA0004", "SYNTHETICA0005"].as_slice(),
    ] {
        assert_eq!(serial_tail_length(&roster(serials)), 4);
    }

    let width = serial_tail_length(&roster(&[A, B, C, "SYNTHETICB0001"]));
    assert_eq!(width, 5);
    for (serial, suffix) in [(A, "A0001"), (B, "A0002"), (C, "A0003")] {
        let label = device_label(&device(Some(serial), None, Connection::Dongle), width);
        assert_eq!(label.serial_tail.as_deref(), Some(suffix));
    }
    assert_eq!(serial_tail("123", 12), "123");
    assert_eq!(serial_tail("prefixaéβ星", 4), "aéβ星");
    assert_eq!(serial_tail_length(&roster(&[A, A])), 4);
}

#[test]
fn label_keeps_current_hub_number_and_suffix_without_using_slot_number() {
    let mut current = device(Some(A), Some(1), Connection::Dongle);
    let original = device_label(&current, 4);
    assert_eq!(original.hub_number, Some(HubNumber(1)));
    assert_eq!(original.serial_tail.as_deref(), Some("0001"));
    current.key = Some(TrackerKey(4));
    assert_eq!(device_label(&current, 4), original);

    current.host = true;
    assert!(device_label(&current, 4).host);
    current.hub_number = Some(HubNumber(3));
    let reconnected = device_label(&current, 4);
    assert_eq!(reconnected.hub_number, Some(HubNumber(3)));
    assert_eq!(reconnected.serial_tail, original.serial_tail);

    current.connection = Connection::Disconnected;
    let detached = device_label(&current, 4);
    assert_eq!(detached.hub_number, None);
    assert!(!detached.host);
    assert_eq!(detached.serial_tail, original.serial_tail);

    let mut unnamed = device(None, Some(12), Connection::Dongle);
    assert_eq!(device_label(&unnamed, 4), TrackerLabel::default());
    unnamed.host = true;
    assert_eq!(
        device_label(&unnamed, 4),
        TrackerLabel {
            host: true,
            ..TrackerLabel::default()
        }
    );
}

fn apply(state: &mut State, ms: i64, event: Event) {
    state.apply(&Timed {
        at: LogTime(ms),
        offset: UtcOffset(0),
        event,
    });
}

fn serial_event(tracker: TrackerKey, serial: &str, number: u16) -> Event {
    Event::SerialKnown {
        tracker,
        serial: Serial(serial.into()),
        hub_number: Some(HubNumber(number)),
        firmware: None,
    }
}

#[test]
fn notice_target_follows_serial_or_connection_and_never_a_reused_slot() {
    let key = TrackerKey(2);
    let mut state = State::default();
    apply(
        &mut state,
        0,
        Event::TrackerAdded {
            tracker: key,
            usb: false,
        },
    );
    apply(&mut state, 1, Event::HostAssigned { tracker: key });
    let original = state.snapshot(LogTime(1));
    let detection = Detection {
        rule: RuleId::R1,
        tracker: Some(key),
        episode: 1,
        serial: None,
        conn: original.host.as_ref().map(|host| host.conn),
        host_wifi_queried: Some(false),
        at: original.at,
        step: Step::Raised,
    };
    let frozen = target_label(&original, &detection);
    let unnamed_host = TrackerLabel {
        host: true,
        ..TrackerLabel::default()
    };
    assert_eq!(frozen, unnamed_host);

    apply(&mut state, 5, serial_event(key, A, 1));
    let known = state.snapshot(LogTime(5));
    let identified = target_label(&known, &detection);
    assert_eq!(identified.hub_number, Some(HubNumber(1)));
    assert_eq!(identified.serial_tail.as_deref(), Some("0001"));
    assert_eq!(host_label(&known), identified);

    apply(
        &mut state,
        10,
        Event::TrackerRemoved {
            tracker: key,
            usb: false,
        },
    );
    apply(
        &mut state,
        11,
        Event::TrackerAdded {
            tracker: key,
            usb: false,
        },
    );
    apply(&mut state, 12, serial_event(key, "SYNTHETICB0001", 3));
    let reused = state.snapshot(LogTime(12));
    assert_eq!(target_label(&reused, &detection), unnamed_host);

    let known_target = Detection {
        serial: Some(Serial(A.into())),
        ..detection
    };
    let detached = target_label(&reused, &known_target);
    assert_eq!(detached.hub_number, None);
    assert_eq!(detached.serial_tail.as_deref(), Some("A0001"));
    assert_eq!(host_label(&reused), detached);
    assert_eq!(frozen, unnamed_host);
}
