use crate::{Pose, Quat, Vec3};

/// Calibration from the remapped tracker coordinates into SteamVR space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    /// Right-handed rotation around SteamVR's vertical axis, in radians.
    pub yaw_rad: f64,
    /// Shift in SteamVR metres.
    pub translation: Vec3,
    pub scale: f64,
}

impl Calibration {
    pub const IDENTITY: Self = Self::new(0.0, Vec3::ZERO, 1.0);

    pub const fn new(yaw_rad: f64, translation: Vec3, scale: f64) -> Self {
        Self {
            yaw_rad,
            translation,
            scale,
        }
    }

    /// Applies uniform scale and translation to position and yaw to the full pose.
    pub fn apply(self, pose: Pose) -> Pose {
        let yaw = Quat::yaw(self.yaw_rad);
        Pose {
            position: yaw.rotate(pose.position) * self.scale + self.translation,
            rotation: yaw * pose.rotation,
        }
    }
}

impl Default for Calibration {
    fn default() -> Self {
        Self::IDENTITY
    }
}
