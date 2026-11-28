use crate::{Pose, Vec3};

/// Validity and pose to send to SteamVR after applying OpenUT's tracking policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeldPose {
    Tracked(Pose),
    RotationOnly(Pose),
    Invalid,
}

impl HeldPose {
    /// The version 1 bridge state code.
    pub const fn state(self) -> u8 {
        match self {
            Self::Tracked(_) => 0,
            Self::RotationOnly(_) => 1,
            Self::Invalid => 2,
        }
    }
}

/// Holds the last tracked position of one tracker in SteamVR coordinates.
#[derive(Default, Debug)]
pub struct PoseHold {
    last_tracked_position: Option<Vec3>,
}

impl PoseHold {
    /// Converts a raw dongle tracking status according to walkthrough §1.6.
    pub fn apply(&mut self, tracking_status: i8, pose: Pose) -> HeldPose {
        match tracking_status {
            2 => {
                self.last_tracked_position = Some(pose.position);
                HeldPose::Tracked(pose)
            }
            3 | 4 => match self.last_tracked_position {
                Some(position) => HeldPose::RotationOnly(Pose { position, ..pose }),
                None => HeldPose::Invalid,
            },
            _ => HeldPose::Invalid,
        }
    }
}
