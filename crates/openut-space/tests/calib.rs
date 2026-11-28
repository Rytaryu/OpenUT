use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};
use std::time::Duration;

use openut_space::calib::{
    CalibrationConfig, CalibrationError, CalibrationResult, Collector, TimedSample,
};
use openut_space::{AxisMap, Mat3, Pose, Quat, Vec3};

const TRACKER: u32 = 7;
const CONTROLLER: u32 = 41;
const START: Duration = Duration::from_secs(10);
const NOW: Duration = Duration::from_secs(18);
const SAMPLE_COUNT: u32 = 150;
const SAMPLE_STEP_MS: u32 = 32;
const SEED: u32 = 0x3f25_b831;

#[derive(Clone, Copy)]
struct Rig {
    axes: AxisMap,
    yaw: f64,
    translation: Vec3,
    scale: f64,
    offset_ms: i32,
    grip_position: Vec3,
    grip_rotation: Quat,
}

impl Rig {
    fn new(axes: AxisMap) -> Self {
        Self {
            axes,
            yaw: 37.0_f64.to_radians(),
            translation: Vec3::new(-0.34, 0.42, 0.22),
            scale: 0.98,
            offset_ms: 45,
            grip_position: Vec3::new(0.06, -0.035, 0.027),
            grip_rotation: pitch(0.38) * roll(-0.27),
        }
    }

    fn controller(self, raw_tracker: Pose, grip_shift: Vec3) -> Pose {
        let mapped_position = self.axes.apply_position(raw_tracker.position);
        let mapped_rotation = self.axes.apply_rotation(raw_tracker.rotation);
        let yaw = Quat::yaw(self.yaw);
        let grip_position = mapped_rotation.rotate(self.grip_position + grip_shift);
        Pose {
            position: yaw.rotate(mapped_position + grip_position) * self.scale + self.translation,
            rotation: yaw * mapped_rotation * self.grip_rotation,
        }
    }

    fn with_equivalent_axes(self, axes: AxisMap) -> Self {
        let relative = axes.matrix() * self.axes.matrix().transpose();
        let turn = relative.rows[0][2].atan2(relative.rows[0][0]);
        let rotation = Quat::yaw(turn);
        Self {
            axes,
            yaw: self.yaw - turn,
            grip_position: rotation.rotate(self.grip_position),
            grip_rotation: rotation * self.grip_rotation,
            ..self
        }
    }

    fn effective_axes(self) -> Mat3 {
        Quat::yaw(self.yaw).to_matrix() * self.axes.matrix()
    }
}

#[derive(Clone, Copy)]
enum Motion {
    WristTurns,
    TranslationOnly,
    OneAxis,
    Independent,
    GripSlip,
}

fn pitch(angle: f64) -> Quat {
    let (sin, cos) = (angle * 0.5).sin_cos();
    Quat::new(cos, sin, 0.0, 0.0)
}

fn roll(angle: f64) -> Quat {
    let (sin, cos) = (angle * 0.5).sin_cos();
    Quat::new(cos, 0.0, 0.0, sin)
}

fn tracker_pose(t: f64, motion: Motion) -> Pose {
    let figure_eight = t * TAU / 3.2;
    let position = Vec3::new(
        0.42 * figure_eight.sin(),
        1.05 + 0.16 * (1.2 * t + 0.3).sin(),
        0.35 * (2.0 * figure_eight).sin(),
    );
    let yaw = Quat::yaw(0.66 * (1.2 * t).sin() + 0.24 * (4.3 * t).sin());
    let rotation = match motion {
        Motion::TranslationOnly => Quat::IDENTITY,
        Motion::OneAxis => yaw,
        _ => {
            yaw * pitch(0.83 * (2.28 * t).sin() + 0.22 * (3.4 * t).cos())
                * roll(0.68 * (1.6 * t + 0.4).sin() + 0.18 * (4.85 * t).sin())
        }
    };
    Pose { position, rotation }
}

fn independent_pose(t: f64, rate: f64) -> Pose {
    let phase = t * rate;
    Pose {
        position: Vec3::new(
            0.3 * phase.sin(),
            1.3 + 0.1 * (0.41 * phase).sin(),
            0.24 * phase.cos(),
        ),
        rotation: pitch(0.78 * phase.sin()) * roll(0.57 * (phase * 1.7 + 0.5).sin()),
    }
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        2.0 * f64::from(self.0) / f64::from(u32::MAX) - 1.0
    }

    fn unit_vector(&mut self) -> Vec3 {
        let vector = Vec3::new(self.next(), self.next(), self.next());
        vector * vector.norm().recip()
    }

    fn apply(&mut self, pose: Pose) -> Pose {
        let position = pose.position + self.unit_vector() * 0.001;
        let axis = self.unit_vector();
        let (sin, cos) = (0.2_f64.to_radians() * 0.5).sin_cos();
        let perturbation = Quat::new(cos, axis.x * sin, axis.y * sin, axis.z * sin);
        Pose {
            position,
            rotation: perturbation * pose.rotation,
        }
    }
}

fn collect(rig: Rig, motion: Motion, seed: Option<u32>, include_distractors: bool) -> Collector {
    let mut collector = Collector::new(START);
    let mut noise = seed.map(Noise);
    for index in 0..SAMPLE_COUNT {
        let controller_ms = 200 + index * SAMPLE_STEP_MS;
        // Sample the tracker on a different part of the cadence to require interpolation.
        let tracker_ms = controller_ms + 11;
        let controller_at = START + Duration::from_millis(u64::from(controller_ms));
        let tracker_at = START
            + Duration::from_millis(
                u64::try_from(i64::from(tracker_ms) - i64::from(rig.offset_ms)).unwrap(),
            );
        let controller_t = f64::from(controller_ms) / 1_000.0;
        let tracker_t = f64::from(tracker_ms) / 1_000.0;
        let mut raw = tracker_pose(tracker_t, motion);
        let mut controller = if matches!(motion, Motion::Independent) {
            independent_pose(controller_t, 6.2)
        } else {
            let grip_shift = if matches!(motion, Motion::GripSlip) && index >= SAMPLE_COUNT / 2 {
                Vec3::new(0.05, 0.0, 0.0)
            } else {
                Vec3::ZERO
            };
            rig.controller(tracker_pose(controller_t, motion), grip_shift)
        };
        if let Some(noise) = &mut noise {
            raw = noise.apply(raw);
            controller = noise.apply(controller);
        }
        collector.add_tracker(
            TRACKER,
            TimedSample {
                at: tracker_at,
                pose: raw,
            },
        );
        collector.add_controller(
            CONTROLLER,
            TimedSample {
                at: controller_at,
                pose: controller,
            },
        );

        if include_distractors {
            for (id, rate) in [(1, 4.4), (3, 7.1)] {
                collector.add_tracker(
                    id,
                    TimedSample {
                        at: tracker_at,
                        pose: independent_pose(tracker_t, rate),
                    },
                );
            }
            collector.add_controller(
                12,
                TimedSample {
                    at: controller_at,
                    pose: independent_pose(controller_t, 5.5),
                },
            );
        }
    }
    collector
}

fn angle_between(actual: Mat3, expected: Mat3) -> f64 {
    let difference = actual * expected.transpose();
    let rows = difference.rows;
    let trace = rows[0][0] + rows[1][1] + rows[2][2];
    ((trace - 1.0) * 0.5).clamp(-1.0, 1.0).acos()
}

fn assert_recovers_rig(result: &CalibrationResult, rig: Rig) {
    assert_eq!(
        result.axis_map,
        rig.axes.group().representative(),
        "{result:#?}"
    );
    let fitted = result.calibration;
    assert!(fitted.yaw_rad > -PI && fitted.yaw_rad <= PI, "{result:#?}");
    let actual = Quat::yaw(fitted.yaw_rad).to_matrix() * result.axis_map.matrix();
    assert!(
        angle_between(actual, rig.effective_axes()) <= 0.5_f64.to_radians(),
        "{result:#?}"
    );
    assert!(
        (fitted.translation - rig.translation).norm() <= 0.01,
        "{result:#?}"
    );
    assert!(
        (fitted.scale / rig.scale - 1.0).abs() <= 0.005,
        "{result:#?}"
    );
    assert!(
        (result.offset_ms - rig.offset_ms).abs() <= 10,
        "{result:#?}"
    );
    assert!(
        result.axis_residual_ratio.is_some_and(|ratio| ratio >= 1.5),
        "{result:#?}"
    );
}

#[test]
fn solves_the_rigid_transform_from_noisy_raw_axes() {
    let rig = Rig::new("-y+z+x".parse().unwrap());
    let collector = collect(rig, Motion::WristTurns, Some(SEED), false);
    let result = collector
        .calibrate(NOW, &CalibrationConfig::default())
        .unwrap();
    assert_recovers_rig(&result, rig);
}

#[test]
fn equivalent_quarter_turn_maps_recover_the_same_canonical_transform() {
    let rig = Rig::new("+x+z+y".parse().unwrap());
    let group = rig.axes.group();
    assert_eq!(rig.axes, group.representative());
    let reference = collect(rig, Motion::WristTurns, Some(SEED), false)
        .calibrate(NOW, &CalibrationConfig::default())
        .unwrap();
    let mut count = 0;
    for axes in AxisMap::all().filter(|axes| axes.group() == group) {
        let equivalent = rig.with_equivalent_axes(axes);
        let result = collect(equivalent, Motion::WristTurns, Some(SEED ^ count), false)
            .calibrate(NOW, &CalibrationConfig::default())
            .unwrap();
        assert_recovers_rig(&result, equivalent);
        let expected = reference.calibration;
        let actual = result.calibration;
        assert_eq!(result.axis_map, reference.axis_map);
        assert!(
            (actual.yaw_rad - expected.yaw_rad).abs() <= 0.5_f64.to_radians(),
            "{axes}"
        );
        assert!(
            (actual.translation - expected.translation).norm() <= 0.01,
            "{axes}"
        );
        assert!(
            (actual.scale / expected.scale - 1.0).abs() <= 0.005,
            "{axes}"
        );
        count += 1;
    }
    assert_eq!(count, 4);
}

#[test]
fn finds_the_correct_group_among_all_twelve_representatives() {
    let groups: BTreeSet<_> = AxisMap::all().map(AxisMap::group).collect();
    assert_eq!(groups.len(), 12);
    for group in groups {
        let rig = Rig::new(group.representative());
        let result = collect(rig, Motion::WristTurns, Some(SEED), false)
            .calibrate(NOW, &CalibrationConfig::default())
            .unwrap_or_else(|error| panic!("axis group {group:?}: {error:?}"));
        assert_eq!(result.axis_map, group.representative());
        assert!(result.calibration.scale > 0.0);
        assert_recovers_rig(&result, rig);
    }
}

#[test]
fn selects_the_one_rigid_pair_among_three_trackers_and_two_controllers() {
    let rig = Rig::new("+z+x+y".parse().unwrap());
    let result = collect(rig, Motion::WristTurns, Some(SEED), true)
        .calibrate(NOW, &CalibrationConfig::default())
        .unwrap();
    assert_eq!((result.tracker, result.controller), (TRACKER, CONTROLLER));
    assert!(result.correlation >= 0.8);
    assert_recovers_rig(&result, rig);
}

#[test]
fn rejects_translation_without_rotation_and_rotation_about_one_axis() {
    let rig = Rig::new(AxisMap::IDENTITY);
    for motion in [Motion::TranslationOnly, Motion::OneAxis] {
        let collector = collect(rig, motion, None, false);
        assert_eq!(
            collector
                .calibrate(NOW, &CalibrationConfig::default())
                .unwrap_err(),
            CalibrationError::MoreRotation
        );
    }
}

#[test]
fn distinguishes_independent_motion_grip_slip_and_out_of_range_scale() {
    let rig = Rig::new(AxisMap::IDENTITY);
    for (settings, motion, expected) in [
        (rig, Motion::Independent, CalibrationError::Uncorrelated),
        (rig, Motion::GripSlip, CalibrationError::GripSlipped),
        (
            Rig { scale: 1.2, ..rig },
            Motion::WristTurns,
            CalibrationError::ScaleOutOfRange,
        ),
    ] {
        let collector = collect(settings, motion, None, false);
        assert_eq!(
            collector
                .calibrate(NOW, &CalibrationConfig::default())
                .unwrap_err(),
            expected
        );
    }
}

#[test]
fn insufficient_motion_times_out_at_twenty_seconds_of_supplied_time() {
    let rig = Rig::new(AxisMap::IDENTITY);
    let mut collector = Collector::new(START);
    for millis in (0_u32..20_000).step_by(SAMPLE_STEP_MS as usize) {
        let at = START + Duration::from_millis(u64::from(millis));
        let raw = tracker_pose(f64::from(millis) / 1000.0, Motion::TranslationOnly);
        collector.add_tracker(TRACKER, TimedSample { at, pose: raw });
        collector.add_controller(
            CONTROLLER,
            TimedSample {
                at,
                pose: rig.controller(raw, Vec3::ZERO),
            },
        );
    }
    let deadline = START + Duration::from_secs(20);
    let config = CalibrationConfig::default();
    let before = deadline - Duration::from_millis(1);
    assert_eq!(
        collector.calibrate(before, &config).unwrap_err(),
        CalibrationError::MoreRotation
    );
    assert_eq!(
        collector.calibrate(deadline, &config).unwrap_err(),
        CalibrationError::TimedOut
    );
}

#[test]
fn fixed_scale_is_exactly_one_and_identical_input_is_deterministic() {
    let rig = Rig {
        scale: 1.01,
        ..Rig::new("+y-z+x".parse().unwrap())
    };
    let config = CalibrationConfig {
        fix_scale: true,
        ..CalibrationConfig::default()
    };
    let collector = collect(rig, Motion::WristTurns, Some(SEED), false);
    let result = collector.calibrate(NOW, &config).unwrap();
    assert_eq!(result.calibration.scale, 1.0);
    assert_eq!(result.axis_map, rig.axes.group().representative());
    let repeated = collect(rig, Motion::WristTurns, Some(SEED), false);
    assert_eq!(repeated.calibrate(NOW, &config).unwrap(), result);
}
