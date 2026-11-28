//! Rigid-motion calibration for one axis-map representative.

use std::f64::consts::{FRAC_PI_2, PI};

use crate::linalg::NormalEquations;
use crate::{AxisMap, Calibration, Pose, Quat, Vec3};

use super::{CalibrationConfig, CalibrationError, RotationProgress, SamplePair};

#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub calibration: Calibration,
    pub position_rms_m: f64,
    pub orientation_spread_rad: f64,
    pub vertical_axis_error: f64,
}

pub(super) fn solve(
    samples: &[SamplePair],
    axis: AxisMap,
    cfg: &CalibrationConfig,
) -> Result<Candidate, CalibrationError> {
    let samples = map_samples(samples, axis).ok_or(CalibrationError::MoreRotation)?;
    let (yaw_rad, vertical_axis_error) =
        fit_yaw(&samples, cfg.min_pair_rotation_rad).ok_or(CalibrationError::MoreRotation)?;
    let yaw = Quat::yaw(yaw_rad);
    let fit = if cfg.fix_scale {
        fit_position::<6>(&samples, yaw, Some(1.0), cfg.min_pivot_ratio)
    } else {
        fit_position::<7>(&samples, yaw, None, cfg.min_pivot_ratio)
    }
    .ok_or(CalibrationError::MoreRotation)?;

    let position_rms_m = position_rms(&samples, yaw, fit);
    let orientation_spread_rad =
        orientation_spread(&samples, yaw).ok_or(CalibrationError::MoreRotation)?;
    if ![position_rms_m, orientation_spread_rad, vertical_axis_error]
        .into_iter()
        .all(f64::is_finite)
    {
        return Err(CalibrationError::MoreRotation);
    }
    Ok(Candidate {
        calibration: Calibration::new(yaw_rad, fit.translation, fit.scale),
        position_rms_m,
        orientation_spread_rad,
        vertical_axis_error,
    })
}

fn map_samples(samples: &[SamplePair], axis: AxisMap) -> Option<Vec<SamplePair>> {
    samples
        .iter()
        .map(|sample| {
            Some(SamplePair {
                tracker: Pose {
                    position: axis.apply_position(sample.tracker.position),
                    rotation: axis.apply_rotation(sample.tracker.rotation.normalized()?),
                },
                controller: Pose {
                    position: sample.controller.position,
                    rotation: sample.controller.rotation.normalized()?,
                },
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
struct AxisAngle {
    axis: Vec3,
    radians: f64,
}

fn relative_rotation(from: Quat, to: Quat) -> Quat {
    to * from.conjugate()
}

fn axis_angle(relative: Quat) -> Option<AxisAngle> {
    let vector = Vec3::new(relative.x, relative.y, relative.z);
    let length = vector.norm();
    if length == 0.0 {
        return None;
    }
    // Choose the shortest rotation, including when a source changes quaternion sign.
    let sign = if relative.w < 0.0 { -1.0 } else { 1.0 };
    Some(AxisAngle {
        axis: Vec3::new(
            sign * (vector.x / length),
            sign * (vector.y / length),
            sign * (vector.z / length),
        ),
        radians: 2.0 * length.atan2(relative.w.abs()),
    })
}

/// Checks that rotations from the first sample cover two distinct axes.
pub(super) fn sufficient_motion(
    rotations: impl IntoIterator<Item = Quat>,
    min_rotation_rad: f64,
    min_axis_separation_rad: f64,
) -> bool {
    rotation_progress(rotations, min_rotation_rad, min_axis_separation_rad)
        == RotationProgress::Ready
}

pub(super) fn rotation_progress(
    rotations: impl IntoIterator<Item = Quat>,
    min_rotation_rad: f64,
    min_axis_separation_rad: f64,
) -> RotationProgress {
    if !(0.0..=PI).contains(&min_rotation_rad)
        || !(0.0..=FRAC_PI_2).contains(&min_axis_separation_rad)
    {
        return RotationProgress::None;
    }
    let mut rotations = rotations.into_iter().filter_map(Quat::normalized);
    let Some(first) = rotations.next() else {
        return RotationProgress::None;
    };
    let maximum_dot = min_axis_separation_rad.cos();
    let mut axes: Vec<Vec3> = Vec::new();
    for rotation in rotations {
        let Some(delta) = axis_angle(relative_rotation(first, rotation)) else {
            continue;
        };
        if delta.radians < min_rotation_rad {
            continue;
        }
        // Opposite directions describe the same axis for the readiness test.
        if axes
            .iter()
            .any(|axis| axis.dot(delta.axis).abs() <= maximum_dot)
        {
            return RotationProgress::Ready;
        }
        axes.push(delta.axis);
    }
    if axes.is_empty() {
        RotationProgress::None
    } else {
        RotationProgress::OneAxis
    }
}

fn fit_yaw(samples: &[SamplePair], min_rotation_rad: f64) -> Option<(f64, f64)> {
    if !(0.0..=PI).contains(&min_rotation_rad) {
        return None;
    }
    let mut cosine = 0.0;
    let mut sine = 0.0;
    let mut vertical_error = 0.0;
    let mut pairs = 0_usize;
    for (index, i) in samples.iter().enumerate() {
        for j in &samples[index + 1..] {
            let tracker = relative_rotation(i.tracker.rotation, j.tracker.rotation);
            let controller = relative_rotation(i.controller.rotation, j.controller.rotation);
            let (Some(tracker), Some(controller)) = (axis_angle(tracker), axis_angle(controller))
            else {
                continue;
            };
            if tracker.radians.min(controller.radians) < min_rotation_rad {
                continue;
            }
            let weight = (tracker.radians + controller.radians) * 0.5;
            let t = tracker.axis;
            let c = controller.axis;
            cosine += weight * (c.x * t.x + c.z * t.z);
            sine += weight * (c.x * t.z - c.z * t.x);
            vertical_error += (c.y - t.y).abs();
            pairs += 1;
        }
    }
    if pairs == 0 || cosine.hypot(sine) <= f64::EPSILON {
        return None;
    }
    let yaw = sine.atan2(cosine);
    let yaw = if yaw <= -PI { PI } else { yaw };
    Some((yaw, vertical_error / pairs as f64))
}

#[derive(Clone, Copy)]
struct PositionFit {
    scale: f64,
    translation: Vec3,
    /// Lever arm multiplied by scale. Keeping this form also permits ranking
    /// negative-scale candidates before axis selection rejects them.
    scaled_lever_arm: Vec3,
}

fn fit_position<const N: usize>(
    samples: &[SamplePair],
    yaw: Quat,
    fixed_scale: Option<f64>,
    min_pivot_ratio: f64,
) -> Option<PositionFit> {
    let mut equations = NormalEquations::<N>::new();
    for sample in samples {
        let p = yaw.rotate(sample.tracker.position);
        let b = sample.controller.position - p * fixed_scale.unwrap_or(0.0);
        let rotation = (yaw * sample.tracker.rotation).to_matrix();
        for (axis, (rot, target)) in rotation.rows.into_iter().zip([b.x, b.y, b.z]).enumerate() {
            // Coefficients of translation, scaled lever arm, and optional scale.
            let p = [p.x, p.y, p.z][axis];
            let row = std::array::from_fn(|column| match column {
                0..=2 => f64::from(u8::from(column == axis)),
                3..=5 => rot[column - 3],
                _ => p,
            });
            equations.add_row(row, target);
        }
    }

    let x = equations.solve(min_pivot_ratio)?;
    Some(PositionFit {
        scale: fixed_scale.unwrap_or(x[N - 1]),
        translation: Vec3::new(x[0], x[1], x[2]),
        scaled_lever_arm: Vec3::new(x[3], x[4], x[5]),
    })
}

fn position_rms(samples: &[SamplePair], yaw: Quat, fit: PositionFit) -> f64 {
    let sum: f64 = samples
        .iter()
        .map(|sample| {
            let model = yaw.rotate(sample.tracker.position) * fit.scale
                + (yaw * sample.tracker.rotation).rotate(fit.scaled_lever_arm)
                + fit.translation;
            let residual = sample.controller.position - model;
            residual.dot(residual)
        })
        .sum();
    (sum / samples.len() as f64).sqrt()
}

fn orientation_spread(samples: &[SamplePair], yaw: Quat) -> Option<f64> {
    let local_rotation = |sample: &SamplePair| {
        (yaw * sample.tracker.rotation).conjugate() * sample.controller.rotation
    };
    let reference = local_rotation(samples.first()?);
    let mut sum = [0.0; 4];
    for sample in samples {
        let q = local_rotation(sample);
        let dot = q.w * reference.w + q.x * reference.x + q.y * reference.y + q.z * reference.z;
        let sign = if dot < 0.0 { -1.0 } else { 1.0 };
        for (sum, component) in sum.iter_mut().zip([q.w, q.x, q.y, q.z]) {
            *sum += sign * component;
        }
    }
    let mean = Quat::new(sum[0], sum[1], sum[2], sum[3]).normalized()?;
    let squared_angles: f64 = samples
        .iter()
        .map(|sample| {
            let q = relative_rotation(mean, local_rotation(sample));
            let length = Vec3::new(q.x, q.y, q.z).norm();
            let angle = 2.0 * length.atan2(q.w.abs());
            angle * angle
        })
        .sum();
    Some((squared_angles / samples.len() as f64).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotation(axis: Vec3, radians: f64) -> Quat {
        let (sin, cos) = (0.5 * radians).sin_cos();
        let v = axis * (sin / axis.norm());
        Quat::new(cos, v.x, v.y, v.z)
    }

    fn synthetic_samples(axis: AxisMap, calibration: Calibration) -> Vec<SamplePair> {
        let lever_arm = Vec3::new(0.11, -0.03, 0.05);
        let relative_orientation = rotation(Vec3::new(0.2, 0.8, -0.4), 0.31);
        let yaw = Quat::yaw(calibration.yaw_rad);
        let undo_map = axis.inverse();
        (0_u32..32)
            .map(|index| {
                let t = f64::from(index) / 31.0 * 7.0;
                let position = Vec3::new(t.sin(), (2.0 * t).sin() * 0.4, t.cos() * 0.2);
                let rx = rotation(Vec3::new(1.0, 0.0, 0.0), 0.9 * (2.3 * t).sin());
                let rz = rotation(Vec3::new(0.0, 0.0, 1.0), 0.7 * (1.7 * t).cos());
                let tracker_orientation = Quat::yaw(0.65 * t.sin()) * rx * rz;
                SamplePair {
                    tracker: Pose {
                        position: undo_map.apply_position(position),
                        rotation: undo_map.apply_rotation(tracker_orientation),
                    },
                    controller: Pose {
                        position: yaw.rotate(position + tracker_orientation.rotate(lever_arm))
                            * calibration.scale
                            + calibration.translation,
                        rotation: yaw * tracker_orientation * relative_orientation,
                    },
                }
            })
            .collect()
    }

    #[test]
    fn recovers_the_yaw_scale_and_translation_with_a_nonzero_lever_arm() {
        let axis: AxisMap = "-z+y+x".parse().unwrap();
        for scale in [0.96, 1.0, -0.96] {
            let expected = Calibration::new(1.07, Vec3::new(0.33, -0.1, 0.61), scale);
            let cfg = CalibrationConfig {
                fix_scale: scale == 1.0,
                ..CalibrationConfig::default()
            };
            let result = solve(&synthetic_samples(axis, expected), axis, &cfg).unwrap();
            assert!((result.calibration.yaw_rad - expected.yaw_rad).abs() < 1e-11);
            assert!((result.calibration.scale - scale).abs() < 1e-11);
            assert!((result.calibration.translation - expected.translation).norm() < 1e-11);
            assert!(result.position_rms_m < 1e-11);
            assert!(result.orientation_spread_rad < 1e-11);
            assert!(result.vertical_axis_error < 1e-11);
            if cfg.fix_scale {
                assert_eq!(result.calibration.scale, 1.0);
            }
        }
    }

    #[test]
    fn yaw_is_canonical_at_a_half_turn() {
        let expected = Calibration::new(-PI, Vec3::new(0.33, -0.1, 0.61), 0.98);
        let result = solve(
            &synthetic_samples(AxisMap::default(), expected),
            AxisMap::default(),
            &CalibrationConfig::default(),
        )
        .unwrap();
        let yaw = result.calibration.yaw_rad;
        assert!(-PI < yaw && yaw <= PI);
        assert!((yaw.abs() - PI).abs() < 1e-11);
    }

    #[test]
    fn readiness_needs_two_axes_and_ignores_quaternion_sign() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let threshold = 30.0_f64.to_radians();
        let separation = 45.0_f64.to_radians();
        let x_rotation = rotation(x, 0.7);
        let negative = Quat::new(-x_rotation.w, -x_rotation.x, -x_rotation.y, -x_rotation.z);
        assert!(!sufficient_motion(
            [Quat::IDENTITY, x_rotation, negative, rotation(x, 1.4)],
            threshold,
            separation
        ));
        assert!(sufficient_motion(
            [Quat::IDENTITY, x_rotation, rotation(y, 0.75)],
            threshold,
            separation
        ));
    }
}
