//! Signed permutations for the device axes and the tracker body's orientation.

use std::fmt;
use std::str::FromStr;

use crate::{Mat3, Quat, Vec3};

/// A direction along one of the axes in the incoming coordinate system.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SignedAxis {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl SignedAxis {
    const POSITIVE: [Self; 3] = [Self::PosX, Self::PosY, Self::PosZ];

    const fn index(self) -> usize {
        match self {
            Self::PosX | Self::NegX => 0,
            Self::PosY | Self::NegY => 1,
            Self::PosZ | Self::NegZ => 2,
        }
    }

    const fn is_negative(self) -> bool {
        matches!(self, Self::NegX | Self::NegY | Self::NegZ)
    }

    const fn flipped(self) -> Self {
        match self {
            Self::PosX => Self::NegX,
            Self::NegX => Self::PosX,
            Self::PosY => Self::NegY,
            Self::NegY => Self::PosY,
            Self::PosZ => Self::NegZ,
            Self::NegZ => Self::PosZ,
        }
    }

    fn from_index(index: usize, negative: bool) -> Self {
        let positive = Self::POSITIVE[index];
        if negative {
            positive.flipped()
        } else {
            positive
        }
    }

    fn parse(sign: u8, axis: u8) -> Result<Self, ParseAxisError> {
        let index = match axis {
            b'x' => 0,
            b'y' => 1,
            b'z' => 2,
            _ => return Err(ParseAxisError::InvalidFormat),
        };
        let negative = match sign {
            b'+' => false,
            b'-' => true,
            _ => return Err(ParseAxisError::InvalidFormat),
        };
        Ok(Self::from_index(index, negative))
    }
}

/// The sign of the axis map's determinant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Handedness {
    Preserving,
    Reversing,
}

/// The four maps that differ only by a horizontal quarter turn share one group.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AxisGroup {
    pub up: SignedAxis,
    pub handedness: Handedness,
}

impl AxisGroup {
    /// Maps the lower-index horizontal source axis to SteamVR's positive x.
    pub fn representative(self) -> AxisMap {
        let up = self.up.index();
        let x = usize::from(up == 0);
        let z = 3 - x - up;
        let mut map = AxisMap([SignedAxis::POSITIVE[x], self.up, SignedAxis::POSITIVE[z]]);
        if map.group().handedness != self.handedness {
            map.0[2] = map.0[2].flipped();
        }
        map
    }
}

/// The signed source axis used for each output coordinate (x, y, then z).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AxisMap([SignedAxis; 3]);

impl AxisMap {
    pub const IDENTITY: Self = Self(SignedAxis::POSITIVE);

    /// Enumerates the 48 signed permutations.
    pub fn all() -> impl Iterator<Item = Self> {
        const PERMUTATIONS: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        PERMUTATIONS.into_iter().flat_map(|indices| {
            (0..8).map(move |signs| {
                Self([
                    SignedAxis::from_index(indices[0], signs & 1 != 0),
                    SignedAxis::from_index(indices[1], signs & 2 != 0),
                    SignedAxis::from_index(indices[2], signs & 4 != 0),
                ])
            })
        })
    }

    pub fn matrix(self) -> Mat3 {
        let mut rows = [[0.0; 3]; 3];
        for (row, axis) in rows.iter_mut().zip(self.0) {
            row[axis.index()] = if axis.is_negative() { -1.0 } else { 1.0 };
        }
        Mat3::new(rows)
    }

    pub fn inverse(self) -> Self {
        // Transposing a signed permutation moves each nonzero entry to its old column.
        let mut axes = SignedAxis::POSITIVE;
        for (row, axis) in self.0.into_iter().enumerate() {
            axes[axis.index()] = SignedAxis::from_index(row, axis.is_negative());
        }
        Self(axes)
    }

    pub fn group(self) -> AxisGroup {
        let [x, y, z] = self.0;
        let odd_permutation =
            (x.index() > y.index()) ^ (x.index() > z.index()) ^ (y.index() > z.index());
        let negative = odd_permutation ^ x.is_negative() ^ y.is_negative() ^ z.is_negative();
        AxisGroup {
            up: y,
            handedness: if negative {
                Handedness::Reversing
            } else {
                Handedness::Preserving
            },
        }
    }

    pub fn apply_position(self, value: Vec3) -> Vec3 {
        let coordinates = [value.x, value.y, value.z];
        let [x, y, z] = self.0.map(|axis| {
            let value = coordinates[axis.index()];
            if axis.is_negative() { -value } else { value }
        });
        Vec3 { x, y, z }
    }

    /// Changes basis as M * R * Mᵀ, including for a reflected basis.
    pub fn apply_rotation(self, value: Quat) -> Quat {
        // The imaginary quaternion components are an axial vector, so a
        // reflection contributes the determinant sign in addition to M.
        let Vec3 { x, y, z } = self.apply_position(Vec3 {
            x: value.x,
            y: value.y,
            z: value.z,
        });
        match self.group().handedness {
            Handedness::Preserving => Quat::new(value.w, x, y, z),
            Handedness::Reversing => Quat::new(value.w, -x, -y, -z),
        }
    }
}

impl Default for AxisMap {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl FromStr for AxisMap {
    type Err = ParseAxisError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let &[sx, x, sy, y, sz, z] = value.as_bytes() else {
            return Err(ParseAxisError::InvalidFormat);
        };
        let x = SignedAxis::parse(sx, x)?;
        let y = SignedAxis::parse(sy, y)?;
        let z = SignedAxis::parse(sz, z)?;
        if x.index() == y.index() || x.index() == z.index() || y.index() == z.index() {
            return Err(ParseAxisError::RepeatedAxis);
        }
        Ok(Self([x, y, z]))
    }
}

impl fmt::Display for AxisMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for axis in self.0 {
            let sign = if axis.is_negative() { '-' } else { '+' };
            write!(f, "{sign}{}", ['x', 'y', 'z'][axis.index()])?;
        }
        Ok(())
    }
}

/// A proper rotation of the tracker body, composed on the right of the pose.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BodyRotation(AxisMap);

impl BodyRotation {
    pub const IDENTITY: Self = Self(AxisMap::IDENTITY);

    /// Enumerates the 24 signed permutations that preserve handedness.
    pub fn all() -> impl Iterator<Item = Self> {
        AxisMap::all()
            .filter(|map| map.group().handedness == Handedness::Preserving)
            .map(Self)
    }

    pub fn matrix(self) -> Mat3 {
        self.0.matrix()
    }

    /// Composes R * N without changing the position.
    pub fn apply_rotation(self, rotation: Quat) -> Quat {
        // BodyRotation can only contain a proper signed permutation.
        let body =
            Quat::from_matrix(self.matrix()).expect("proper signed permutation is a rotation");
        rotation * body
    }
}

impl Default for BodyRotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl FromStr for BodyRotation {
    type Err = ParseAxisError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let map: AxisMap = value.parse()?;
        if map.group().handedness == Handedness::Reversing {
            return Err(ParseAxisError::Reflection);
        }
        Ok(Self(map))
    }
}

impl fmt::Display for BodyRotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseAxisError {
    InvalidFormat,
    RepeatedAxis,
    Reflection,
}

impl fmt::Display for ParseAxisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => f.write_str("expected three signed axes such as +x+y+z"),
            Self::RepeatedAxis => f.write_str("each source axis must appear once"),
            Self::Reflection => f.write_str("body rotation must preserve handedness"),
        }
    }
}

impl std::error::Error for ParseAxisError {}
