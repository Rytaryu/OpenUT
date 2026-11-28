use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

use openut_space::{Calibration, HeldPose, Pose, PoseHold, Quat, Vec3};

fn pose(position: Vec3, rotation: Quat) -> Pose {
    Pose { position, rotation }
}

fn assert_vec(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).norm() < 1e-12,
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn calibration_identity_preserves_position_and_rotation() {
    let original = pose(Vec3::new(1.0, 2.0, 3.0), Quat::yaw(FRAC_PI_2));
    assert_eq!(Calibration::default().apply(original), original);
}

#[test]
fn calibration_scales_rotates_and_translates_position_and_premultiplies_yaw() {
    let around_x = Quat::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0, 0.0);
    let calibration = Calibration::new(FRAC_PI_2, Vec3::new(5.0, -1.0, 4.0), 2.0);
    let result = calibration.apply(pose(Vec3::new(1.0, 2.0, 3.0), around_x));
    assert_vec(result.position, Vec3::new(11.0, 3.0, 2.0));
    assert_vec(
        result.rotation.rotate(Vec3::new(0.0, 1.0, 0.0)),
        Vec3::new(1.0, 0.0, 0.0),
    );
}

#[test]
fn first_loss_is_invalid_until_a_tracked_position_is_available() {
    let mut hold = PoseHold::default();
    let sample = pose(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY);
    for status in [-1, 0, 1, 3, 4, 5, i8::MAX] {
        let result = hold.apply(status, sample);
        assert_eq!(result, HeldPose::Invalid);
        assert_eq!(result.state(), 2);
    }
}

#[test]
fn loss_holds_the_last_tracked_position_and_recovery_replaces_it() {
    let mut hold = PoseHold::default();
    let initial = pose(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY);
    let tracked = hold.apply(2, initial);
    assert_eq!(tracked, HeldPose::Tracked(initial));
    assert_eq!(tracked.state(), 0);

    let next_rotation = Quat::yaw(FRAC_PI_2);
    let loss = pose(Vec3::new(50.0, 60.0, 70.0), next_rotation);
    for status in [3, 4] {
        let result = hold.apply(status, loss);
        assert_eq!(
            result,
            HeldPose::RotationOnly(pose(initial.position, next_rotation))
        );
        assert_eq!(result.state(), 1);
    }
    assert_eq!(hold.apply(1, loss), HeldPose::Invalid);

    let recovered = pose(Vec3::new(2.0, 3.0, 4.0), next_rotation);
    assert_eq!(hold.apply(2, recovered), HeldPose::Tracked(recovered));
    assert_eq!(
        hold.apply(3, loss),
        HeldPose::RotationOnly(pose(recovered.position, next_rotation))
    );
    assert_eq!(PoseHold::default().apply(3, loss), HeldPose::Invalid);
}
