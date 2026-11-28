//! Coordinate transforms and tracking-state conversion for OpenUT poses.

mod axis;
pub mod calib;
pub mod fingerprint;
mod hold;
mod linalg;
mod math;
mod transform;

pub use axis::{AxisGroup, AxisMap, BodyRotation, Handedness, ParseAxisError, SignedAxis};
pub use hold::{HeldPose, PoseHold};
pub use math::{Mat3, Quat, Vec3};
pub use transform::Calibration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
}
