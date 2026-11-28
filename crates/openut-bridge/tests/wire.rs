use openut_bridge::{
    DecodeError, DeviceEntry, HEADER_LEN, MAX_DEVICES, MAX_NAME_LEN, MAX_PACKET_LEN,
    POSE_PACKET_LEN, POSE_PAYLOAD_LEN, Packet, Payload, Pose, TrackingState, ValueError, decode,
    encode,
};

const DEVICE_LIST: [u8; 28] = [
    0x4f, 0x55, 0x54, 0x42, // magic
    0x01, 0x00, 0x01, 0x00, // version, kind, flags
    0x67, 0x45, 0x23, 0x01, // session
    0xef, 0xcd, 0xab, 0x89, // seq
    0x02, // device count
    0x00, 0x05, 0x4c, 0x61, 0x62, 0x20, 0x41, // device 0, "Lab A"
    0x07, 0x02, 0x7e, 0x7a, // device 7, "~z"
];

const POSE: [u8; 74] = [
    0x4f, 0x55, 0x54, 0x42, // magic
    0x01, 0x00, 0x02, 0x00, // version, kind, flags
    0x67, 0x45, 0x23, 0x01, // session
    0xef, 0xcd, 0xab, 0x89, // seq
    0x07, 0x01, // device, state
    0x40, 0x30, 0x20, 0x10, // age_us
    0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x60, 0x40, // position
    0x00, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0xbf, // rotation w, x
    0xf3, 0x04, 0x35, 0x3f, 0x00, 0x00, 0x00, 0x00, // rotation y, z
    0x00, 0x00, 0x80, 0xc0, 0x00, 0x00, 0xb0, 0x40, 0x00, 0x00, 0xc0, 0x40, // velocity
    0x00, 0x00, 0xe0, 0x40, 0x00, 0x00, 0x04, 0xc1, 0x00, 0x00, 0x18,
    0x41, // angular velocity
];

fn packet(payload: Payload) -> Packet {
    Packet {
        session: 0x0123_4567,
        seq: 0x89ab_cdef,
        payload,
    }
}

fn sample_pose() -> Pose {
    Pose {
        device: 7,
        state: TrackingState::RotationOnly,
        age_us: 0x1020_3040,
        position: [1.0, -2.0, 3.5],
        rotation: [0.5, -0.5, std::f32::consts::FRAC_1_SQRT_2, 0.0],
        velocity: [-4.0, 5.5, 6.0],
        angular_velocity: [7.0, -8.25, 9.5],
    }
}

fn device(device: u8, name: impl Into<String>) -> DeviceEntry {
    DeviceEntry {
        device,
        name: name.into(),
    }
}

#[test]
fn device_list_has_the_specified_byte_layout() {
    let expected = packet(Payload::DeviceList(vec![
        device(0, "Lab A"),
        device(7, "~z"),
    ]));
    assert_eq!(decode(&DEVICE_LIST), Ok(expected.clone()));
    assert_eq!(encode(&expected).unwrap(), DEVICE_LIST);
}

#[test]
fn pose_has_the_specified_byte_layout() {
    let expected = packet(Payload::Pose(sample_pose()));
    assert_eq!(decode(&POSE), Ok(expected.clone()));
    assert_eq!(encode(&expected).unwrap(), POSE);
}

#[test]
fn device_lists_round_trip_at_wire_size_boundaries() {
    assert_eq!((HEADER_LEN, MAX_DEVICES, MAX_NAME_LEN), (16, 8, 32));
    let largest = (0..8)
        .map(|slot| device(slot, format!(" {slot}{}~", "!".repeat(29))))
        .collect();
    for entries in [vec![], vec![device(0, "")], largest] {
        let wire_len = 16
            + 1
            + entries
                .iter()
                .map(|entry| 2 + entry.name.len())
                .sum::<usize>();
        let expected = Packet {
            session: 1,
            seq: u32::MAX,
            payload: Payload::DeviceList(entries),
        };
        let bytes = encode(&expected).unwrap();
        assert_eq!(bytes.len(), wire_len);
        assert_eq!(decode(&bytes), Ok(expected));
    }
    assert_eq!(MAX_PACKET_LEN, 16 + 1 + 8 * (2 + 32));
}

#[test]
fn poses_round_trip_at_the_exact_wire_size_for_every_state() {
    assert_eq!(
        (HEADER_LEN, POSE_PAYLOAD_LEN, POSE_PACKET_LEN),
        (16, 58, 74)
    );
    for (state_byte, state) in [
        (0, TrackingState::Tracked),
        (1, TrackingState::RotationOnly),
        (2, TrackingState::Invalid),
    ] {
        let expected = packet(Payload::Pose(Pose {
            state,
            age_us: u32::MAX,
            ..sample_pose()
        }));
        let bytes = encode(&expected).unwrap();
        assert_eq!(bytes.len(), 74);
        assert_eq!(bytes[17], state_byte);
        assert_eq!(decode(&bytes), Ok(expected));
    }
}

fn reject_changed_byte(valid: &[u8], offset: usize, byte: u8, error: DecodeError) {
    let mut bytes = valid.to_vec();
    bytes[offset] = byte;
    assert_eq!(decode(&bytes), Err(error), "byte {offset} = {byte:#04x}");
}

#[test]
fn rejects_invalid_header_fields() {
    for valid in [DEVICE_LIST.as_slice(), POSE.as_slice()] {
        reject_changed_byte(valid, 0, b'X', DecodeError::BadMagic);
        reject_changed_byte(valid, 5, 2, DecodeError::UnsupportedVersion(0x0201));
        reject_changed_byte(valid, 6, 3, DecodeError::UnknownKind(3));
        reject_changed_byte(valid, 7, 0x80, DecodeError::UnsupportedFlags(0x80));
        let mut bytes = valid.to_vec();
        bytes[8..12].fill(0);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::InvalidValue(ValueError::ZeroSession))
        );
    }
    let mut invalid = packet(Payload::DeviceList(vec![]));
    invalid.session = 0;
    assert_eq!(encode(&invalid), Err(ValueError::ZeroSession));
}

#[test]
fn rejects_every_truncated_prefix_and_extra_bytes() {
    for valid in [DEVICE_LIST.as_slice(), POSE.as_slice()] {
        for len in 0..valid.len() {
            assert_eq!(
                decode(&valid[..len]),
                Err(DecodeError::Truncated),
                "length {len}"
            );
        }
        let mut bytes = valid.to_vec();
        bytes.push(0);
        assert_eq!(decode(&bytes), Err(DecodeError::TrailingBytes));
    }
}

#[test]
fn rejects_device_counts_and_numbers_above_the_wire_limits() {
    let mut bytes = DEVICE_LIST[..16].to_vec();
    bytes.push(9);
    for slot in 0..9 {
        bytes.extend([slot % 8, 1, b'A' + slot]);
    }
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::InvalidValue(ValueError::TooManyDevices(9)))
    );
    let entries = (0..9)
        .map(|slot| device(slot % 8, format!("Tracker {slot}")))
        .collect();
    assert_eq!(
        encode(&packet(Payload::DeviceList(entries))),
        Err(ValueError::TooManyDevices(9))
    );

    let error = DecodeError::InvalidValue(ValueError::InvalidDevice(8));
    reject_changed_byte(&DEVICE_LIST, 17, 8, error);
    reject_changed_byte(&POSE, 16, 8, error);
    assert_eq!(
        encode(&packet(Payload::DeviceList(vec![device(8, "Lab A")]))),
        Err(ValueError::InvalidDevice(8)),
    );
    assert_eq!(
        encode(&packet(Payload::Pose(Pose {
            device: 8,
            ..sample_pose()
        }))),
        Err(ValueError::InvalidDevice(8)),
    );
}

#[test]
fn rejects_names_that_are_too_long_or_not_printable_ascii() {
    let mut bytes = DEVICE_LIST[..16].to_vec();
    bytes.extend([1, 0, 33]);
    bytes.extend([b'A'; 33]);
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::InvalidValue(ValueError::NameTooLong(33)))
    );
    let entries = vec![device(0, "A".repeat(33))];
    assert_eq!(
        encode(&packet(Payload::DeviceList(entries))),
        Err(ValueError::NameTooLong(33))
    );

    for byte in [0x00, 0x0a, 0x7f, 0x80, 0xff] {
        reject_changed_byte(
            &DEVICE_LIST,
            19,
            byte,
            DecodeError::InvalidValue(ValueError::InvalidName),
        );
    }
    for name in ["A\0B", "A\nB", "\x7f", "é"] {
        assert_eq!(
            encode(&packet(Payload::DeviceList(vec![device(0, name)]))),
            Err(ValueError::InvalidName),
        );
    }
}

#[test]
fn rejects_unknown_tracking_states() {
    reject_changed_byte(&POSE, 17, 3, DecodeError::InvalidState(3));
    reject_changed_byte(&POSE, 17, 0xff, DecodeError::InvalidState(0xff));
}

#[test]
fn rejects_non_finite_pose_components() {
    for offset in (22..74).step_by(4) {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bytes = POSE;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(
                decode(&bytes),
                Err(DecodeError::InvalidValue(ValueError::NonFinite)),
                "non-finite value at byte {offset}",
            );
        }
    }

    let invalid = [
        Pose {
            position: [f32::NAN, 0.0, 0.0],
            ..sample_pose()
        },
        Pose {
            rotation: [1.0, 0.0, f32::INFINITY, 0.0],
            ..sample_pose()
        },
        Pose {
            velocity: [0.0, f32::NEG_INFINITY, 0.0],
            ..sample_pose()
        },
        Pose {
            angular_velocity: [0.0, 0.0, f32::NAN],
            ..sample_pose()
        },
    ];
    for pose in invalid {
        assert_eq!(
            encode(&packet(Payload::Pose(pose))),
            Err(ValueError::NonFinite)
        );
    }
}

#[test]
fn checks_quaternion_magnitude_on_both_sides_of_unit_length() {
    for magnitude in [0.991, 1.009] {
        let expected = packet(Payload::Pose(Pose {
            rotation: [magnitude, 0.0, 0.0, 0.0],
            ..sample_pose()
        }));
        assert_eq!(decode(&encode(&expected).unwrap()), Ok(expected));
    }
    for magnitude in [0.0_f32, 0.989, 1.011, f32::MAX] {
        let rotation = [magnitude, 0.0, 0.0, 0.0];
        let invalid = packet(Payload::Pose(Pose {
            rotation,
            ..sample_pose()
        }));
        assert_eq!(encode(&invalid), Err(ValueError::NonUnitRotation));

        let mut bytes = POSE;
        let (encoded_rotation, _) = bytes[34..50].as_chunks_mut::<4>();
        for (component, value) in encoded_rotation.iter_mut().zip(rotation) {
            *component = value.to_le_bytes();
        }
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::InvalidValue(ValueError::NonUnitRotation))
        );
    }
}

fn next_random(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

#[test]
fn decoding_ten_thousand_varied_byte_strings_does_not_panic() {
    let mut seed = 0x6d2b_79f5;
    for case in 0..10_000 {
        let kind = case % 4;
        let len = if kind == 3 {
            POSE_PACKET_LEN
        } else {
            next_random(&mut seed) as usize % (MAX_PACKET_LEN + 65)
        };
        let mut bytes: Vec<_> = (0..len).map(|_| next_random(&mut seed) as u8).collect();
        // Valid headers in some cases allow the randomized payload bytes to be parsed.
        if let Some(header) = bytes.get_mut(..16) {
            match kind {
                1 => {
                    header.copy_from_slice(&DEVICE_LIST[..16]);
                    if let Some(count) = bytes.get_mut(16) {
                        *count %= 9;
                    }
                }
                2 => header.copy_from_slice(&POSE[..16]),
                3 => {
                    header.copy_from_slice(&POSE[..16]);
                    bytes[16] %= 8;
                    bytes[17] %= 3;
                }
                _ => {}
            }
        }
        let _ = decode(&bytes);
    }
}
