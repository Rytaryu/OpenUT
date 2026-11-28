//! Version 1 of the OpenUT app-to-driver packet format.

use crate::{
    DecodeError, DeviceEntry, HEADER_LEN, MAX_DEVICES, MAX_NAME_LEN, POSE_PACKET_LEN,
    POSE_PAYLOAD_LEN, Packet, Payload, Pose, TrackingState, ValueError,
};

const MAGIC: [u8; 4] = *b"OUTB";
const VERSION: u16 = 1;
const DEVICE_LIST: u8 = 1;
const POSE: u8 = 2;

/// Encodes a packet after checking its wire format constraints.
pub fn encode(packet: &Packet) -> Result<Vec<u8>, ValueError> {
    validate_session(packet.session)?;
    let (kind, size) = match &packet.payload {
        Payload::DeviceList(devices) => {
            validate_count(devices.len())?;
            let mut size = HEADER_LEN + 1;
            for entry in devices {
                validate_device(entry.device)?;
                validate_name(entry.name.as_bytes())?;
                size += 2 + entry.name.len();
            }
            (DEVICE_LIST, size)
        }
        Payload::Pose(pose) => {
            validate_pose(pose)?;
            (POSE, POSE_PACKET_LEN)
        }
    };

    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&[kind, 0]);
    bytes.extend_from_slice(&packet.session.to_le_bytes());
    bytes.extend_from_slice(&packet.seq.to_le_bytes());
    match &packet.payload {
        Payload::DeviceList(devices) => {
            bytes.push(devices.len() as u8);
            for entry in devices {
                bytes.extend_from_slice(&[entry.device, entry.name.len() as u8]);
                bytes.extend_from_slice(entry.name.as_bytes());
            }
        }
        Payload::Pose(pose) => {
            bytes.extend_from_slice(&[pose.device, pose.state as u8]);
            bytes.extend_from_slice(&pose.age_us.to_le_bytes());
            for value in pose_components(pose) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(bytes)
}

/// Decodes one complete packet. Trailing bytes are rejected.
pub fn decode(bytes: &[u8]) -> Result<Packet, DecodeError> {
    if bytes.len() < HEADER_LEN {
        return Err(DecodeError::Truncated);
    }
    let mut input = Reader { bytes };
    if input.array::<4>()? != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = u16::from_le_bytes(input.array()?);
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let kind = input.byte()?;
    if !matches!(kind, DEVICE_LIST | POSE) {
        return Err(DecodeError::UnknownKind(kind));
    }
    let flags = input.byte()?;
    if flags != 0 {
        return Err(DecodeError::UnsupportedFlags(flags));
    }
    let session = u32::from_le_bytes(input.array()?);
    validate_session(session)?;
    let seq = u32::from_le_bytes(input.array()?);
    let payload = if kind == DEVICE_LIST {
        Payload::DeviceList(decode_devices(input)?)
    } else {
        Payload::Pose(decode_pose(input)?)
    };

    Ok(Packet {
        session,
        seq,
        payload,
    })
}

fn decode_devices(mut input: Reader<'_>) -> Result<Vec<DeviceEntry>, DecodeError> {
    let count = usize::from(input.byte()?);
    validate_count(count)?;
    // Validate every entry and the complete packet length while names remain borrowed.
    let mut entries = [None; MAX_DEVICES];
    for entry in &mut entries[..count] {
        let device = input.byte()?;
        validate_device(device)?;
        let len = usize::from(input.byte()?);
        validate_name_len(len)?;
        let name = input.take(len)?;
        validate_name(name)?;
        let name = std::str::from_utf8(name).map_err(|_| ValueError::InvalidName)?;
        *entry = Some((device, name));
    }
    if !input.bytes.is_empty() {
        return Err(DecodeError::TrailingBytes);
    }
    Ok(entries
        .into_iter()
        .flatten()
        .map(|(device, name)| DeviceEntry {
            device,
            name: name.to_owned(),
        })
        .collect())
}

fn decode_pose(mut input: Reader<'_>) -> Result<Pose, DecodeError> {
    use std::cmp::Ordering;
    match input.bytes.len().cmp(&POSE_PAYLOAD_LEN) {
        Ordering::Less => return Err(DecodeError::Truncated),
        Ordering::Greater => return Err(DecodeError::TrailingBytes),
        Ordering::Equal => {}
    }

    let device = input.byte()?;
    let state = match input.byte()? {
        0 => TrackingState::Tracked,
        1 => TrackingState::RotationOnly,
        2 => TrackingState::Invalid,
        state => return Err(DecodeError::InvalidState(state)),
    };
    let pose = Pose {
        device,
        state,
        age_us: u32::from_le_bytes(input.array()?),
        position: input.floats()?,
        rotation: input.floats()?,
        velocity: input.floats()?,
        angular_velocity: input.floats()?,
    };
    validate_pose(&pose)?;
    Ok(pose)
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let (value, rest) = self
            .bytes
            .split_at_checked(len)
            .ok_or(DecodeError::Truncated)?;
        self.bytes = rest;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (value, rest) = self
            .bytes
            .split_first_chunk::<N>()
            .ok_or(DecodeError::Truncated)?;
        self.bytes = rest;
        Ok(*value)
    }

    fn byte(&mut self) -> Result<u8, DecodeError> {
        let [value] = self.array()?;
        Ok(value)
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N], DecodeError> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = f32::from_le_bytes(self.array()?);
        }
        Ok(values)
    }
}

fn validate_session(session: u32) -> Result<(), ValueError> {
    if session == 0 {
        return Err(ValueError::ZeroSession);
    }
    Ok(())
}

fn validate_count(count: usize) -> Result<(), ValueError> {
    if count > MAX_DEVICES {
        return Err(ValueError::TooManyDevices(count));
    }
    Ok(())
}

fn validate_device(device: u8) -> Result<(), ValueError> {
    if usize::from(device) >= MAX_DEVICES {
        return Err(ValueError::InvalidDevice(device));
    }
    Ok(())
}

fn validate_name_len(len: usize) -> Result<(), ValueError> {
    if len > MAX_NAME_LEN {
        return Err(ValueError::NameTooLong(len));
    }
    Ok(())
}

fn validate_name(name: &[u8]) -> Result<(), ValueError> {
    validate_name_len(name.len())?;
    if !name.iter().all(|byte| matches!(byte, b' '..=b'~')) {
        return Err(ValueError::InvalidName);
    }
    Ok(())
}

fn pose_components(pose: &Pose) -> impl Iterator<Item = f32> + '_ {
    [
        pose.position.as_slice(),
        pose.rotation.as_slice(),
        pose.velocity.as_slice(),
        pose.angular_velocity.as_slice(),
    ]
    .into_iter()
    .flatten()
    .copied()
}

fn validate_pose(pose: &Pose) -> Result<(), ValueError> {
    validate_device(pose.device)?;
    if pose_components(pose).any(|value| !value.is_finite()) {
        return Err(ValueError::NonFinite);
    }
    let norm = pose
        .rotation
        .into_iter()
        .map(|value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    if (norm - 1.0).abs() > 0.01 {
        return Err(ValueError::NonUnitRotation);
    }
    Ok(())
}
