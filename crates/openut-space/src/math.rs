//! Spatial arithmetic in double precision, with right-handed rotations.

use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn norm(self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;

    fn mul(self, scalar: f64) -> Self {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quat {
    pub const IDENTITY: Self = Self::new(1.0, 0.0, 0.0, 0.0);

    /// Constructs a quaternion with its scalar part first.
    pub const fn new(w: f64, x: f64, y: f64, z: f64) -> Self {
        Self { w, x, y, z }
    }

    /// Right-handed rotation around +Y. Positive angles turn +X toward −Z.
    pub fn yaw(radians: f64) -> Self {
        let (sin, cos) = (radians * 0.5).sin_cos();
        Self::new(cos, 0.0, sin, 0.0)
    }

    /// Normalizes a finite, nonzero quaternion, including subnormal components.
    pub fn normalized(self) -> Option<Self> {
        let components = [self.w, self.x, self.y, self.z];
        if !components.into_iter().all(f64::is_finite) {
            return None;
        }
        let scale = components.into_iter().map(f64::abs).fold(0.0, f64::max);
        if scale == 0.0 {
            return None;
        }
        let q = Self::new(
            self.w / scale,
            self.x / scale,
            self.y / scale,
            self.z / scale,
        );
        Some(q.scaled(q.dot(q).sqrt().recip()))
    }

    /// Returns the conjugate, which is the inverse for a unit quaternion.
    pub const fn conjugate(self) -> Self {
        Self::new(self.w, -self.x, -self.y, -self.z)
    }

    /// Rotates a vector with a unit quaternion.
    pub fn rotate(self, vector: Vec3) -> Vec3 {
        let imaginary = Vec3::new(self.x, self.y, self.z);
        let twice_cross = imaginary.cross(vector) * 2.0;
        vector + twice_cross * self.w + imaginary.cross(twice_cross)
    }

    /// Converts a unit quaternion to a matrix that rotates column vectors.
    pub fn to_matrix(self) -> Mat3 {
        let Self { w, x, y, z } = self;
        Mat3::new([
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ])
    }

    /// Converts a proper rotation matrix to a unit quaternion. The input may
    /// have small rounding errors in its determinant and row dot products.
    pub fn from_matrix(matrix: Mat3) -> Option<Self> {
        // Allow rotation matrices that were rounded to single precision.
        const TOLERANCE: f64 = 1e-6;
        let orthonormal = (matrix * matrix.transpose())
            .rows
            .iter()
            .flatten()
            .zip(Mat3::IDENTITY.rows.into_iter().flatten())
            .all(|(actual, expected)| (actual - expected).abs() <= TOLERANCE);
        let proper = orthonormal && (matrix.determinant() - 1.0).abs() <= TOLERANCE;
        if !proper {
            return None;
        }

        let [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]] = matrix.rows;
        let trace = xx + yy + zz;
        // Solve for a large component to avoid division by a value near zero
        // even when the rotation is close to a half turn.
        let q = if trace > 0.0 {
            let s = 2.0 * (1.0 + trace).sqrt();
            Self::new(0.25 * s, (zy - yz) / s, (xz - zx) / s, (yx - xy) / s)
        } else if xx >= yy && xx >= zz {
            let s = 2.0 * (1.0 + xx - yy - zz).sqrt();
            Self::new((zy - yz) / s, 0.25 * s, (xy + yx) / s, (xz + zx) / s)
        } else if yy >= zz {
            let s = 2.0 * (1.0 + yy - xx - zz).sqrt();
            Self::new((xz - zx) / s, (xy + yx) / s, 0.25 * s, (yz + zy) / s)
        } else {
            let s = 2.0 * (1.0 + zz - xx - yy).sqrt();
            Self::new((yx - xy) / s, (xz + zx) / s, (yz + zy) / s, 0.25 * s)
        };
        q.normalized()
    }

    /// Interpolates along the shorter rotation between two orientations.
    /// Both endpoints must be finite and nonzero, and `t` must be in [0, 1].
    pub fn slerp(self, other: Self, t: f64) -> Option<Self> {
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        let start = self.normalized()?;
        let mut end = other.normalized()?;
        let mut dot = start.dot(end);
        if dot < 0.0 {
            end = end.scaled(-1.0);
            dot = -dot;
        }

        let (start_weight, end_weight) = if dot >= 1.0 - 8.0 * f64::EPSILON {
            // At tiny angles, normalized linear interpolation avoids dividing
            // by the sine of an angle near zero.
            (1.0 - t, t)
        } else {
            let angle = dot.acos();
            let sin = angle.sin();
            (((1.0 - t) * angle).sin() / sin, (t * angle).sin() / sin)
        };
        Self::new(
            start.w * start_weight + end.w * end_weight,
            start.x * start_weight + end.x * end_weight,
            start.y * start_weight + end.y * end_weight,
            start.z * start_weight + end.z * end_weight,
        )
        .normalized()
    }

    fn dot(self, other: Self) -> f64 {
        self.w * other.w + self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn scaled(self, scalar: f64) -> Self {
        Self::new(
            self.w * scalar,
            self.x * scalar,
            self.y * scalar,
            self.z * scalar,
        )
    }
}

impl Mul for Quat {
    type Output = Self;

    /// Hamilton product. For unit quaternions, applies the right rotation first.
    fn mul(self, other: Self) -> Self {
        Self::new(
            self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
            self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
        )
    }
}

/// A 3×3 matrix stored by rows. Vectors are multiplied as columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    pub rows: [[f64; 3]; 3],
}

impl Mat3 {
    pub const IDENTITY: Self = Self::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    pub const fn new(rows: [[f64; 3]; 3]) -> Self {
        Self { rows }
    }

    pub fn transpose(self) -> Self {
        Self::new(std::array::from_fn(|row| {
            std::array::from_fn(|column| self.rows[column][row])
        }))
    }

    pub fn determinant(self) -> f64 {
        let [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]] = self.rows;
        xx * (yy * zz - yz * zy) - xy * (yx * zz - yz * zx) + xz * (yx * zy - yy * zx)
    }
}

impl Mul for Mat3 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                self.rows[row][0] * other.rows[0][column]
                    + self.rows[row][1] * other.rows[1][column]
                    + self.rows[row][2] * other.rows[2][column]
            })
        }))
    }
}

impl Mul<Vec3> for Mat3 {
    type Output = Vec3;

    fn mul(self, vector: Vec3) -> Vec3 {
        let [x, y, z] = self.rows.map(|[x, y, z]| Vec3::new(x, y, z).dot(vector));
        Vec3::new(x, y, z)
    }
}
