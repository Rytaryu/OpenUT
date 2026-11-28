use std::io::Cursor;

use openut_dongle::decode::{Decoder, DeviceInfoField, DongleEvent, SlotStatus, TrackerMessage};
use openut_dongle::half::to_f32;
use openut_dongle::record::{Record, RecordReader, TrackerLabel};

const ADDRESS: [u8; 6] = [0x71, 0x12, 0x33, 0x54, 0xa5, 0xc6];

fn tracker_report(address: [u8; 6], kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut report = vec![0x28, 0x34, 0x12];
    report.extend_from_slice(&address);
    report.extend_from_slice(&kind.to_le_bytes());
    report.push(u8::try_from(payload.len()).expect("synthetic payload is short"));
    report.extend_from_slice(payload);
    report
}

fn pairing_report(address: [u8; 6], unpaired: u8) -> Vec<u8> {
    let mut report = vec![0x18, 12, 0x22, 0x44, 0x66, unpaired];
    report.extend_from_slice(&address);
    report
}

fn append_half(payload: &mut Vec<u8>, bits: [u16; 4]) {
    payload.extend(bits.into_iter().flat_map(u16::to_le_bytes));
}

#[test]
fn reads_pose_fields_and_reorders_rotation() {
    let mut payload = vec![3, 0xa9];
    payload.extend([1.25_f32, -2.5, 4.0].into_iter().flat_map(f32::to_le_bytes));
    // w, z, y, x on the wire, with distinct values to catch ordering errors.
    append_half(&mut payload, [0x3c00, 0xc000, 0x4200, 0x4400]);
    payload.extend(
        [0x3800_u16, 0xb800, 0x3c00]
            .into_iter()
            .flat_map(u16::to_le_bytes),
    );
    append_half(&mut payload, [0x4000, 0xc200, 0x4400, 0x4500]);
    payload.extend_from_slice(&[0xff, 0xa3, 0xd4]);
    let mut report = tracker_report(ADDRESS, 0x0110, &payload);
    report.extend_from_slice(&[0x12, 0x34]); // Report padding is outside the declared payload.

    let event = Decoder::default().decode(&report);
    let DongleEvent::Pose { tracker, sample } = event else {
        panic!("expected a pose, got {event:?}");
    };
    assert_eq!(tracker, TrackerLabel('A'));
    assert_eq!(sample.slot, 3);
    assert_eq!(sample.buttons_raw, 0xa9);
    assert_eq!(sample.position_m, [1.25, -2.5, 4.0]);
    assert_eq!(sample.rotation_wxyz, [1.0, 4.0, 3.0, -2.0]);
    assert_eq!(sample.acceleration, [0.5, -0.5, 1.0]);
    assert_eq!(sample.angular_velocity, [2.0, -3.0, 4.0, 5.0]);
    assert_eq!(sample.tracking_status, -1);
    assert_eq!(sample.extra, [0xa3, 0xd4]);
    assert!(!format!("{sample:?}").contains("[163, 212]"));
}

#[test]
fn converts_ieee_half_values_without_losing_signed_zero() {
    assert_eq!(to_f32(0x0000).to_bits(), 0.0_f32.to_bits());
    assert_eq!(to_f32(0x8000).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(to_f32(0x3c00), 1.0);
    assert_eq!(to_f32(0xc000), -2.0);
    assert_eq!(to_f32(0x0001), 2.0_f32.powi(-24));
    assert_eq!(to_f32(0x03ff), 1023.0 * 2.0_f32.powi(-24));
    assert_eq!(to_f32(0x7c00), f32::INFINITY);
    assert_eq!(to_f32(0xfc00), f32::NEG_INFINITY);
    assert!(to_f32(0x7e00).is_nan());
}

#[test]
fn reads_all_five_rf_slots_from_the_input_report() {
    let mut report = vec![0x1e, 29, 0, 0, 0xa1, 0xb2, 0xc3, 0xd4, 0xe5];
    let expected = [
        (3, 0x0102_u16, 0b0000_0001),
        (5, 0x2345, 0b0000_0010),
        (8, 0x3456, 0b0001_1000),
        (6, 0x4567, 0b0010_0000),
        (3, 0xabcd, 0b0100_0100),
    ];
    for (status, timeout, flags) in expected {
        report.push(status);
        report.extend_from_slice(&timeout.to_le_bytes());
        report.push(flags);
    }

    let event = Decoder::default().decode(&report);
    let DongleEvent::RfStatus { slots } = event else {
        panic!("expected RF status, got {event:?}");
    };
    for (
        SlotStatus {
            status,
            timeout_ms,
            flags,
        },
        (expected_status, timeout, expected_flags),
    ) in slots.into_iter().zip(expected)
    {
        assert_eq!(
            (status, timeout_ms, flags),
            (expected_status, timeout, expected_flags)
        );
    }
}

#[test]
fn unifies_tracker_identity_across_the_two_report_layouts() {
    let mut decoder = Decoder::default();
    let data = tracker_report(ADDRESS, 0x006e, &[0x01, 0x02]);
    assert!(matches!(
        decoder.decode(&data),
        DongleEvent::OtherTrackerData {
            tracker: TrackerLabel('A'),
            kind: 0x006e,
            len: 2
        }
    ));
    assert!(matches!(
        decoder.decode(&pairing_report(ADDRESS, 0)),
        DongleEvent::Pair {
            tracker: TrackerLabel('A'),
            unpaired: false
        }
    ));

    let mut different_first_byte = ADDRESS;
    different_first_byte[0] ^= 0x0f;
    let mut unrelated_fields_changed = pairing_report(different_first_byte, 1);
    unrelated_fields_changed[2..5].copy_from_slice(&[0x95, 0x86, 0x77]);
    assert!(matches!(
        decoder.decode(&unrelated_fields_changed),
        DongleEvent::Pair {
            tracker: TrackerLabel('A'),
            unpaired: true
        }
    ));

    let mut shifted = [0x18, 13, 0x22, 0x44, 0x66, 0, 0xf9, 0, 0, 0, 0, 0, 0];
    shifted[7..13].copy_from_slice(&ADDRESS);
    assert!(matches!(
        decoder.decode(&shifted),
        DongleEvent::Pair {
            tracker: TrackerLabel('B'),
            unpaired: false
        }
    ));
    assert!(matches!(
        decoder.decode(&data),
        DongleEvent::OtherTrackerData {
            tracker: TrackerLabel('A'),
            ..
        }
    ));
}

#[test]
fn tolerates_one_leading_zero_before_a_known_type() {
    let mut decoder = Decoder::default();
    let mut padded = vec![0];
    padded.extend(pairing_report(ADDRESS, 0));
    assert!(matches!(
        decoder.decode(&padded),
        DongleEvent::Pair {
            tracker: TrackerLabel('A'),
            unpaired: false
        }
    ));

    let unclassified = [0, 0xfe, 3, 7];
    assert!(matches!(
        decoder.decode(&unclassified),
        DongleEvent::Unknown {
            report_type: 0,
            len: 4
        }
    ));
}

#[test]
fn preserves_uninterpreted_report_types_without_showing_payload() {
    let bytes = [0x29, 4, 0xa1, 0xb2];
    assert!(matches!(
        Decoder::default().decode(&bytes),
        DongleEvent::Unknown {
            report_type: 0x29,
            len: 4
        }
    ));
}

#[test]
fn counts_truncated_and_invalid_reports_as_malformed() {
    let mut decoder = Decoder::default();
    let pair = pairing_report(ADDRESS, 0);
    let mut length_too_short = pair.clone();
    length_too_short[1] = 11;
    let mut length_too_long = pair.clone();
    length_too_long[1] = 13;
    let mut short_payload = tracker_report(ADDRESS, 0x0101, b"WS");
    short_payload[11] += 1;

    for (bytes, report_type) in [
        (&pair[..11], 0x18),
        (&length_too_short, 0x18),
        (&length_too_long, 0x18),
        (&pairing_report(ADDRESS, 2), 0x18),
        (&short_payload, 0x28),
        (&tracker_report(ADDRESS, 0x0110, &[0; 36]), 0x28),
        (&tracker_report(ADDRESS, 0x0101, &[0xff]), 0x28),
        (&[0x1e, 29, 0, 0], 0x1e),
    ] {
        let event = decoder.decode(bytes);
        assert!(
            matches!(event, DongleEvent::Malformed { report_type: kind, .. } if kind == report_type),
            "unexpected event: {event:?}"
        );
    }
    assert!(matches!(
        decoder.decode(&pair),
        DongleEvent::Pair {
            tracker: TrackerLabel('A'),
            ..
        }
    ));
}

#[test]
fn handles_ten_thousand_deterministic_arbitrary_inputs() {
    let mut seed = 0x720b_4e38_u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed.to_be_bytes()[0]
    };
    let mut decoder = Decoder::default();
    for index in 0..10_000 {
        let len = usize::from(next() % 81);
        let mut report = (0..len).map(|_| next()).collect::<Vec<_>>();
        if let Some(first) = report.first_mut() {
            *first = match index % 5 {
                0 => 0x18,
                1 => 0x1e,
                2 => 0x28,
                3 => 0,
                _ => *first,
            };
        }
        let _ = decoder.decode(&report);
    }
}

fn text_message(decoder: &mut Decoder, text: &str) -> TrackerMessage {
    let event = decoder.decode(&tracker_report(ADDRESS, 0x0101, text.as_bytes()));
    let DongleEvent::Text { tracker, message } = event else {
        panic!("expected tracker text, got {event:?}");
    };
    assert_eq!(tracker, TrackerLabel('A'));
    message
}

#[test]
fn decodes_documented_device_info_fields() {
    let mut decoder = Decoder::default();
    for (text, expected_field, expected_value) in [
        ("2NADSsynthetic-id", DeviceInfoField::Nads, "synthetic-id"),
        (
            "NASSsynthetic-factory",
            DeviceInfoField::Nass,
            "synthetic-factory",
        ),
        ("NASIsynthetic-sku", DeviceInfoField::Nasi, "synthetic-sku"),
        (
            "NAPIsynthetic-board",
            DeviceInfoField::Napi,
            "synthetic-board",
        ),
        ("NAV10.2.3", DeviceInfoField::Nav1, "0.2.3"),
        ("NARI1", DeviceInfoField::Nari, "1"),
        ("NAGN1,2,3", DeviceInfoField::Nagn, "1,2,3"),
        (
            "NANAsynthetic-intro",
            DeviceInfoField::Nana,
            "synthetic-intro",
        ),
        (
            "NADNsynthetic-name",
            DeviceInfoField::Nadn,
            "synthetic-name",
        ),
        (
            "NADIsynthetic-device",
            DeviceInfoField::Nadi,
            "synthetic-device",
        ),
    ] {
        let message = text_message(&mut decoder, text);
        let TrackerMessage::DeviceInfo { field, value } = message else {
            panic!("unexpected text: {message:?}");
        };
        assert_eq!(field, expected_field);
        assert_eq!(value.expose(), expected_value);
        if expected_value.starts_with("synthetic-") {
            assert!(!format!("{value:?} {value}").contains(expected_value));
        }
    }
}

#[test]
fn decodes_documented_status_and_control_notices() {
    let mut decoder = Decoder::default();
    assert!(matches!(
        text_message(&mut decoder, "NAZZ"),
        TrackerMessage::Nazz
    ));
    assert!(matches!(
        text_message(&mut decoder, "4MS-1,6"),
        TrackerMessage::MapStatus { a: -1, b: 6 }
    ));
    assert!(matches!(
        text_message(&mut decoder, "LS-2,3,44"),
        TrackerMessage::LambdaStatus(values) if values == [-2, 3, 44]
    ));
    assert!(matches!(
        text_message(&mut decoder, "LPtest-attribute"),
        TrackerMessage::SlamAttributes(value) if value.expose() == "test-attribute"
    ));
    assert!(matches!(
        text_message(&mut decoder, "P61:5,-9876543210"),
        TrackerMessage::SlamStatus {
            key: 5,
            value: -9_876_543_210
        }
    ));
    assert!(matches!(
        text_message(&mut decoder, "P82:0:1200"),
        TrackerMessage::SlamError { code: 1200 }
    ));
    assert!(matches!(
        text_message(&mut decoder, "WS"),
        TrackerMessage::WifiInfoRequest
    ));
    assert!(matches!(
        text_message(&mut decoder, "WC5"),
        TrackerMessage::WifiConnected { ok: true }
    ));
    assert!(matches!(
        text_message(&mut decoder, "WC0"),
        TrackerMessage::WifiConnected { ok: false }
    ));
    assert!(matches!(
        text_message(&mut decoder, "APF"),
        TrackerMessage::PowerOff
    ));
    assert!(matches!(
        text_message(&mut decoder, "APR"),
        TrackerMessage::Reset
    ));
    assert!(matches!(
        text_message(&mut decoder, "DEC1200"),
        TrackerMessage::DeviceError(value) if value.expose() == "1200"
    ));
    assert!(matches!(
        text_message(&mut decoder, "2ZZunclassified-test"),
        TrackerMessage::Unknown { prefix } if prefix == "ZZ"
    ));
}

#[test]
fn redacts_private_notices_and_does_not_retain_host_credentials() {
    let mut decoder = Decoder::default();
    for (text, private) in [
        (
            "NADSsynthetic-id-never-rendered",
            "synthetic-id-never-rendered",
        ),
        (
            "Csynthetic-upper-calibration",
            "synthetic-upper-calibration",
        ),
        (
            "csynthetic-lower-calibration",
            "synthetic-lower-calibration",
        ),
        ("Wssynthetic-network-label", "synthetic-network-label"),
        (
            "Wpsynthetic-network-credential",
            "synthetic-network-credential",
        ),
        ("Wtsynthetic-network-setting", "synthetic-network-setting"),
    ] {
        let event = decoder.decode(&tracker_report(ADDRESS, 0x0101, text.as_bytes()));
        let debug = format!("{event:?}");
        let DongleEvent::Text {
            tracker: TrackerLabel('A'),
            message,
        } = event
        else {
            panic!("expected tracker text, got {debug}");
        };
        let value = match message {
            TrackerMessage::DeviceInfo { value, .. }
            | TrackerMessage::Calibration(value)
            | TrackerMessage::WifiSetting(value) => value,
            _ => panic!("expected a redacted value"),
        };
        assert!(value.expose().contains(private));
        assert!(!debug.contains(private));
        assert!(!format!("{value}").contains(private));
    }

    let message = text_message(
        &mut decoder,
        "3WHsynthetic-host-network,synthetic-host-credential,2412",
    );
    assert!(matches!(message, TrackerMessage::WifiHostInfo));
    assert!(!format!("{message:?}").contains("synthetic-host"));
}

#[test]
fn rejects_broken_numeric_notices_and_only_strips_one_slot_digit() {
    let mut decoder = Decoder::default();
    for text in ["MS1", "LS1,,3", "P61:256,1", "P82:0:-1", "WCx"] {
        let event = decoder.decode(&tracker_report(ADDRESS, 0x0101, text.as_bytes()));
        assert!(matches!(
            event,
            DongleEvent::Malformed {
                report_type: 0x28,
                ..
            }
        ));
    }
    assert!(matches!(
        text_message(&mut decoder, "12WS"),
        TrackerMessage::Unknown { prefix } if prefix == "2W"
    ));
}

#[test]
fn decodes_saved_synthetic_reports_in_record_order() {
    let fixture = include_str!("../../../tests/fixtures/dongle/m2-2-decode.jsonl");
    let mut reader = RecordReader::new(Cursor::new(fixture)).expect("read synthetic header");
    let mut decoder = Decoder::default();
    let mut times = Vec::new();
    let mut events = Vec::new();
    while let Some(record) = reader.read_record().expect("read synthetic report") {
        let Record::Input(input) = record else {
            panic!("fixture contains input reports only");
        };
        times.push(input.mono_us);
        events.push(decoder.decode(&input.bytes));
    }
    assert_eq!(times, [10, 20, 30, 40]);
    assert_eq!(reader.stats().malformed, 0);
    assert!(matches!(
        events.as_slice(),
        [
            DongleEvent::Pair {
                tracker: TrackerLabel('A'),
                unpaired: false
            },
            DongleEvent::Text {
                tracker: TrackerLabel('A'),
                message: TrackerMessage::WifiInfoRequest
            },
            DongleEvent::RfStatus { .. },
            DongleEvent::Pair {
                tracker: TrackerLabel('A'),
                unpaired: true
            },
        ]
    ));
}
