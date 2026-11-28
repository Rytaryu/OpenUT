//! SteamVR pose values, independent of the OpenVR ABI.

use std::time::Duration;

use openut_bridge::{Pose as BridgePose, TrackingState};

const IDENTITY_ROTATION: [f64; 4] = [1.0, 0.0, 0.0, 0.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingResult {
    Uninitialized,
    RunningOk,
    RotationOnly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub time_offset: f64,
    pub world_from_driver_rotation: [f64; 4],
    pub driver_from_head_rotation: [f64; 4],
    pub position: [f64; 3],
    pub rotation: [f64; 4],
    pub velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
    pub result: TrackingResult,
    pub valid: bool,
    pub connected: bool,
}

impl Pose {
    /// Converts a decoded pose, including the time it spent pending in the driver.
    pub fn from_packet(pose: BridgePose, driver_delay: Duration) -> Self {
        let result = match pose.state {
            TrackingState::Tracked => TrackingResult::RunningOk,
            TrackingState::RotationOnly => TrackingResult::RotationOnly,
            TrackingState::Invalid => TrackingResult::Uninitialized,
        };
        Self {
            time_offset: -(f64::from(pose.age_us) / 1_000_000.0 + driver_delay.as_secs_f64()),
            position: pose.position.map(f64::from),
            rotation: pose.rotation.map(f64::from),
            velocity: pose.velocity.map(f64::from),
            angular_velocity: pose.angular_velocity.map(f64::from),
            result,
            valid: pose.state != TrackingState::Invalid,
            ..Self::invalid(true)
        }
    }

    pub const fn invalid(connected: bool) -> Self {
        Self {
            time_offset: 0.0,
            world_from_driver_rotation: IDENTITY_ROTATION,
            driver_from_head_rotation: IDENTITY_ROTATION,
            position: [0.0; 3],
            rotation: IDENTITY_ROTATION,
            velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            result: TrackingResult::Uninitialized,
            valid: false,
            connected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_tracking_state_and_pose_field() {
        let packet = BridgePose {
            device: 2,
            state: TrackingState::Tracked,
            age_us: 20_000,
            position: [1.25, -2.0, 3.5],
            rotation: [0.5, 0.5, -0.5, 0.5],
            velocity: [0.125, 0.25, 0.5],
            angular_velocity: [0.5, 0.25, -0.125],
        };
        for (state, result, valid) in [
            (TrackingState::Tracked, TrackingResult::RunningOk, true),
            (
                TrackingState::RotationOnly,
                TrackingResult::RotationOnly,
                true,
            ),
            (TrackingState::Invalid, TrackingResult::Uninitialized, false),
        ] {
            let pose = Pose::from_packet(BridgePose { state, ..packet }, Duration::from_millis(5));
            assert_eq!(pose.result, result);
            assert_eq!(pose.valid, valid);
            assert!(pose.connected);
            assert_eq!(pose.world_from_driver_rotation, IDENTITY_ROTATION);
            assert_eq!(pose.driver_from_head_rotation, IDENTITY_ROTATION);
            assert_eq!(pose.position, packet.position.map(f64::from));
            assert_eq!(pose.rotation, packet.rotation.map(f64::from));
            assert_eq!(pose.velocity, packet.velocity.map(f64::from));
            assert_eq!(
                pose.angular_velocity,
                packet.angular_velocity.map(f64::from)
            );
            assert!((pose.time_offset + 0.025).abs() < f64::EPSILON);
        }
    }
}
