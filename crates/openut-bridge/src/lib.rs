//! OpenUT bridge packets as specified in walkthrough design §1.4.1.

mod wire;

use std::{error::Error, fmt};

pub use wire::{decode, encode};

pub const HEADER_LEN: usize = 16;
pub const POSE_PAYLOAD_LEN: usize = 58;
pub const POSE_PACKET_LEN: usize = HEADER_LEN + POSE_PAYLOAD_LEN;
pub const MAX_DEVICES: usize = 8;
pub const MAX_NAME_LEN: usize = 32;
pub const MAX_PACKET_LEN: usize = HEADER_LEN + 1 + MAX_DEVICES * (2 + MAX_NAME_LEN);

#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    /// Nonzero ID selected by the sender for this session.
    pub session: u32,
    pub seq: u32,
    pub payload: Payload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    DeviceList(Vec<DeviceEntry>),
    Pose(Pose),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceEntry {
    pub device: u8,
    /// Printable ASCII SteamVR device name.
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TrackingState {
    Tracked = 0,
    RotationOnly = 1,
    Invalid = 2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub device: u8,
    pub state: TrackingState,
    /// Time in microseconds between receiving the pose and sending the packet.
    pub age_us: u32,
    pub position: [f32; 3],
    /// Unit quaternion in wire order: w, x, y, z.
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

/// A field value invalid in either direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueError {
    ZeroSession,
    TooManyDevices(usize),
    InvalidDevice(u8),
    NameTooLong(usize),
    InvalidName,
    NonFinite,
    NonUnitRotation,
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ZeroSession => f.write_str("session ID is zero"),
            Self::TooManyDevices(count) => write!(f, "device list has {count} entries"),
            Self::InvalidDevice(device) => write!(f, "device index {device} is out of range"),
            Self::NameTooLong(length) => write!(f, "device name has {length} bytes"),
            Self::InvalidName => f.write_str("device name is not printable ASCII"),
            Self::NonFinite => f.write_str("pose contains a non-finite component"),
            Self::NonUnitRotation => {
                f.write_str("rotation is outside the unit quaternion tolerance")
            }
        }
    }
}

impl Error for ValueError {}

/// Rejection reason for an incoming version 1 packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    BadMagic,
    UnsupportedVersion(u16),
    UnknownKind(u8),
    UnsupportedFlags(u8),
    Truncated,
    TrailingBytes,
    InvalidState(u8),
    InvalidValue(ValueError),
}

impl From<ValueError> for DecodeError {
    fn from(error: ValueError) -> Self {
        Self::InvalidValue(error)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::BadMagic => f.write_str("invalid bridge packet signature"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported bridge version {version}"),
            Self::UnknownKind(kind) => write!(f, "unknown bridge packet kind {kind}"),
            Self::UnsupportedFlags(flags) => {
                write!(f, "unsupported bridge packet flags {flags:#04x}")
            }
            Self::Truncated => f.write_str("bridge packet is truncated"),
            Self::TrailingBytes => f.write_str("bridge packet has trailing bytes"),
            Self::InvalidState(state) => write!(f, "invalid tracking state {state}"),
            Self::InvalidValue(error) => error.fmt(f),
        }
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidValue(error) => Some(error),
            _ => None,
        }
    }
}
