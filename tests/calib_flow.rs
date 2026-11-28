use std::f64::consts::TAU;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use openut::calib_file;
use openut::calib_flow::{
    CalibrationFailure, CalibrationFlow, CalibrationRecord, CalibrationStatus,
};
use openut::run::pipeline::Pipeline;
use openut_bridge::{Packet, Payload, TrackingState};
use openut_dongle::decode::{DongleEvent, PoseSample};
use openut_dongle::record::TrackerLabel;
use openut_space::calib::{CalibrationConfig, CalibrationError, TimedSample};
use openut_space::fingerprint::SpaceFingerprint;
use openut_space::{AxisMap, BodyRotation, Calibration, Pose, Quat, Vec3};

const CONTROLLER: u32 = 41;
const START: Duration = Duration::from_secs(10);
const SOLVE_AT: Duration = Duration::from_secs(18);
const TIMESTAMP: u64 = 1_795_927_800;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("openut-calib-flow-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn calibration(&self) -> PathBuf {
        self.0.join("calibration.json")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy)]
struct Rig {
    axes: AxisMap,
    transform: Calibration,
}

impl Rig {
    fn new(axes: AxisMap) -> Self {
        Self {
            axes: axes.group().representative(),
            transform: Calibration::new(37.0_f64.to_radians(), Vec3::new(-0.34, 0.42, 0.22), 0.98),
        }
    }

    fn controller_pose(self, raw: Pose) -> Pose {
        let tracker = expected_pose(raw, self.axes, self.transform);
        let grip_rotation = pitch(0.38) * roll(-0.27);
        Pose {
            position: tracker.position
                + tracker.rotation.rotate(Vec3::new(0.06, -0.035, 0.027)) * self.transform.scale,
            rotation: tracker.rotation * grip_rotation,
        }
    }
}

fn pitch(angle: f64) -> Quat {
    let (sin, cos) = (angle * 0.5).sin_cos();
    Quat::new(cos, sin, 0.0, 0.0)
}

fn roll(angle: f64) -> Quat {
    let (sin, cos) = (angle * 0.5).sin_cos();
    Quat::new(cos, 0.0, 0.0, sin)
}

fn tracker_pose(t: f64) -> Pose {
    // Same figure-eight and wrist turns as the K3-1 synthetic calibration case.
    let phase = t * TAU / 3.2;
    Pose {
        position: Vec3::new(
            0.42 * phase.sin(),
            1.05 + 0.16 * (1.2 * t + 0.3).sin(),
            0.35 * (2.0 * phase).sin(),
        ),
        rotation: Quat::yaw(0.66 * (1.2 * t).sin() + 0.24 * (4.3 * t).sin())
            * pitch(0.83 * (2.28 * t).sin() + 0.22 * (3.4 * t).cos())
            * roll(0.68 * (1.6 * t + 0.4).sin() + 0.18 * (4.85 * t).sin()),
    }
}

fn expected_pose(raw: Pose, axes: AxisMap, transform: Calibration) -> Pose {
    transform.apply(Pose {
        position: axes.apply_position(raw.position),
        rotation: axes.apply_rotation(raw.rotation),
    })
}

fn pose_event(raw: Pose) -> DongleEvent {
    DongleEvent::Pose {
        tracker: TrackerLabel('A'),
        sample: PoseSample {
            slot: 1,
            tracking_status: 2,
            position_m: [raw.position.x, raw.position.y, raw.position.z].map(|v| v as f32),
            rotation_wxyz: [
                raw.rotation.w,
                raw.rotation.x,
                raw.rotation.y,
                raw.rotation.z,
            ]
            .map(|v| v as f32),
            buttons_raw: 0,
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 4],
            extra: Vec::new(),
        },
    }
}

fn at_us(at: Duration) -> u64 {
    u64::try_from(at.as_micros()).unwrap()
}

fn pipeline(settings: AxisMap) -> Pipeline {
    Pipeline::new(
        NonZeroU32::new(17).unwrap(),
        0,
        settings,
        BodyRotation::IDENTITY,
    )
}

fn apply_calibration(pipeline: &mut Pipeline, flow: &CalibrationFlow) {
    let (axis_map, calibration) = flow.active();
    pipeline.set_calibration(axis_map, calibration);
}

fn bridge_pose(pipeline: &mut Pipeline, at: Duration, raw: Pose) -> openut_bridge::Pose {
    let packets = pipeline.apply_event(at_us(at), at_us(at), &pose_event(raw));
    let packet = packets
        .iter()
        .find(|packet| matches!(&packet.payload, Payload::Pose(_)))
        .expect("tracker generates a bridge pose packet");
    let decoded: Packet = openut_bridge::decode(&openut_bridge::encode(packet).unwrap()).unwrap();
    let Payload::Pose(pose) = decoded.payload else {
        panic!("expected a pose payload");
    };
    assert_eq!(pose.state, TrackingState::Tracked);
    pose
}

fn assert_pose(actual: openut_bridge::Pose, expected: Pose) {
    let [x, y, z] = actual.position.map(f64::from);
    let position = Vec3::new(x, y, z);
    assert!(
        (position - expected.position).norm() < 0.01,
        "{position:?} vs {expected:?}"
    );

    let [w, x, y, z] = actual.rotation.map(f64::from);
    let rotation = Quat::new(w, x, y, z).normalized().unwrap();
    let relative = rotation * expected.rotation.conjugate();
    let angle = 2.0
        * Vec3::new(relative.x, relative.y, relative.z)
            .norm()
            .atan2(relative.w.abs());
    assert!(
        angle < 0.5_f64.to_radians(),
        "orientation error {}°",
        angle.to_degrees()
    );
}

fn fingerprint(universe: u64) -> SpaceFingerprint {
    SpaceFingerprint::new(
        universe,
        Pose {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        },
        Some([2.5, 3.0]),
    )
    .unwrap()
}

fn prior_record() -> CalibrationRecord {
    CalibrationRecord {
        axis_map: "-z+x+y"
            .parse::<AxisMap>()
            .unwrap()
            .group()
            .representative(),
        calibration: Calibration::new(-0.3, Vec3::new(0.7, -0.2, 0.1), 1.02),
        position_rms_m: 0.003,
        sample_count: 120,
        offset_ms: -15,
        created_at_unix_s: TIMESTAMP - 1_000,
        fingerprint: fingerprint(31),
    }
}

fn solve(flow: &mut CalibrationFlow, pipeline: &mut Pipeline, rig: Rig) -> CalibrationRecord {
    flow.set_fingerprint(Some(fingerprint(31)));
    assert!(flow.start(START));
    assert!(flow.is_collecting());

    for index in 0..150_u64 {
        let controller_ms = 200 + index * 32;
        let tracker_ms = controller_ms + 11;
        let received = START + Duration::from_millis(tracker_ms - 45);
        let raw = tracker_pose(Duration::from_millis(tracker_ms).as_secs_f64());
        let (_, paired_sample) =
            pipeline.apply_event_with_sample(at_us(received), at_us(received), &pose_event(raw));
        let (tracker, sample) = paired_sample.expect("valid tracked dongle sample");
        assert_eq!(sample.at, received);
        assert!((sample.pose.position - raw.position).norm() < 1e-6);
        flow.add_tracker(tracker, sample);

        let controller_at = Duration::from_millis(controller_ms);
        flow.add_controller(
            CONTROLLER,
            TimedSample {
                at: START + controller_at,
                pose: rig.controller_pose(tracker_pose(controller_at.as_secs_f64())),
            },
        );
    }

    let result = flow
        .tick(SOLVE_AT, TIMESTAMP)
        .expect("valid samples finish calibration");
    assert!(matches!(
        flow.view().status,
        CalibrationStatus::Completed { .. }
    ));
    assert_eq!(result.axis_map, rig.axes);
    assert_eq!(flow.active(), (result.axis_map, result.calibration));
    result
}

#[test]
fn recalibration_uses_raw_samples_and_applies_the_saved_pair_after_restart() {
    let settings = "+x+z-y".parse::<AxisMap>().unwrap();
    let rig = Rig::new("+z+y+x".parse().unwrap());
    let prior = prior_record();
    assert_ne!(settings, AxisMap::IDENTITY);
    assert_ne!(prior.axis_map.group(), settings.group());
    assert_ne!(rig.axes.group(), settings.group());
    assert_ne!(rig.axes.group(), prior.axis_map.group());

    let scratch = Scratch::new();
    let path = scratch.calibration();
    calib_file::write(&path, &prior).unwrap();
    let mut flow = CalibrationFlow::new(
        CalibrationConfig::default(),
        settings,
        calib_file::read(&path).unwrap(),
    );
    let mut before_restart = pipeline(settings);
    apply_calibration(&mut before_restart, &flow);
    let raw = tracker_pose(0.21);
    assert_pose(
        bridge_pose(&mut before_restart, START - Duration::from_millis(1), raw),
        expected_pose(raw, prior.axis_map, prior.calibration),
    );

    let result = solve(&mut flow, &mut before_restart, rig);
    apply_calibration(&mut before_restart, &flow);
    calib_file::write(&path, &result).unwrap();
    let saved = calib_file::read(&path)
        .unwrap()
        .expect("saved result loads");
    assert_eq!(saved.axis_map, result.axis_map);
    assert_eq!(saved.sample_count, result.sample_count);
    assert_eq!(saved.offset_ms, result.offset_ms);
    assert_eq!(saved.created_at_unix_s, result.created_at_unix_s);
    assert_eq!(saved.fingerprint, result.fingerprint);
    assert!((saved.position_rms_m - result.position_rms_m).abs() < 1e-12);
    assert!((saved.calibration.yaw_rad - result.calibration.yaw_rad).abs() < 1e-12);
    assert!((saved.calibration.translation - result.calibration.translation).norm() < 1e-12);
    assert!((saved.calibration.scale - result.calibration.scale).abs() < 1e-12);
    let saved_pair = (saved.axis_map, saved.calibration);

    let restarted = CalibrationFlow::new(CalibrationConfig::default(), settings, Some(saved));
    let mut after_restart = pipeline(settings);
    apply_calibration(&mut after_restart, &restarted);
    for (index, t) in [0.35, 1.25, 3.7].into_iter().enumerate() {
        let at = SOLVE_AT + Duration::from_millis(32 * (index as u64 + 1));
        let raw = tracker_pose(t);
        let expected = expected_pose(raw, rig.axes, rig.transform);
        let sent = bridge_pose(&mut before_restart, at, raw);
        assert_pose(sent, expected);
        assert_eq!(sent, bridge_pose(&mut after_restart, at, raw));
    }

    let mut clean_flow =
        CalibrationFlow::new(CalibrationConfig::default(), AxisMap::IDENTITY, None);
    let clean = solve(&mut clean_flow, &mut pipeline(AxisMap::IDENTITY), rig);
    calib_file::write(&path, &clean).unwrap();
    let clean = calib_file::read(&path)
        .unwrap()
        .expect("saved clean result loads");
    assert_eq!(saved_pair, (clean.axis_map, clean.calibration));
}

#[test]
fn using_the_configured_axis_group_does_not_apply_the_axis_map_twice() {
    let settings = "+x+z-y".parse::<AxisMap>().unwrap();
    let rig = Rig::new(settings);
    let mut flow = CalibrationFlow::new(CalibrationConfig::default(), settings, None);
    let mut pipeline = pipeline(settings);
    let record = solve(&mut flow, &mut pipeline, rig);
    assert_eq!(record.axis_map, settings.group().representative());
    assert_ne!(record.axis_map, AxisMap::IDENTITY);
    apply_calibration(&mut pipeline, &flow);
    for (index, t) in [0.3, 1.8].into_iter().enumerate() {
        let at = SOLVE_AT + Duration::from_millis(32 * (index as u64 + 1));
        let raw = tracker_pose(t);
        assert_pose(
            bridge_pose(&mut pipeline, at, raw),
            expected_pose(raw, rig.axes, rig.transform),
        );
    }
}

#[test]
fn fingerprint_changes_cancellation_and_timeout_keep_the_previous_pair() {
    let settings = "+x+z-y".parse::<AxisMap>().unwrap();
    let saved = prior_record();
    let expected_pair = (saved.axis_map, saved.calibration);
    let mut flow = CalibrationFlow::new(CalibrationConfig::default(), settings, Some(saved));
    flow.set_fingerprint(Some(fingerprint(32)));
    assert_eq!(flow.view().status, CalibrationStatus::NeedsRecalibration);
    assert!(flow.view().has_calibration);
    assert!(flow.view().needs_recalibration);
    assert_eq!(flow.active(), expected_pair);

    assert!(flow.start(START));
    assert_eq!(flow.active(), expected_pair);
    flow.cancel();
    assert!(!flow.is_collecting());
    assert_eq!(flow.active(), expected_pair);

    assert!(flow.start(START));
    let deadline = START + Duration::from_secs(20);
    assert!(flow.tick(deadline, TIMESTAMP).is_none());
    assert_eq!(
        flow.view().status,
        CalibrationStatus::Failed(CalibrationFailure::Fit(CalibrationError::TimedOut)),
    );
    assert_eq!(flow.active(), expected_pair);

    let mut pipeline = pipeline(settings);
    apply_calibration(&mut pipeline, &flow);
    let raw = tracker_pose(2.2);
    assert_pose(
        bridge_pose(&mut pipeline, deadline, raw),
        expected_pose(raw, expected_pair.0, expected_pair.1),
    );
}
