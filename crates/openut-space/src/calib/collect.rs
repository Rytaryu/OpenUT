//! In-memory sampling, controller pairing and offset estimation.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::{Pose, Vec3};

use super::solve::{rotation_progress, sufficient_motion};
use super::{CalibrationConfig, CalibrationError, RotationProgress};

const MIN_CORRELATION_POINTS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedSample {
    /// Time relative to the caller's common monotonic origin.
    pub at: Duration,
    pub pose: Pose,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SamplePair {
    /// The raw tracker pose, with no axis map or body rotation applied.
    pub tracker: Pose,
    pub controller: Pose,
}

pub(super) struct PairSelection {
    pub tracker: u32,
    pub controller: u32,
    pub offset_ms: i32,
    pub correlation: f64,
    pub pairs: Vec<SamplePair>,
}

/// Collects tracked samples from one calibration attempt.
pub struct Collector {
    started_at: Duration,
    trackers: BTreeMap<u32, Vec<TimedSample>>,
    controllers: BTreeMap<u32, Vec<TimedSample>>,
}

impl Collector {
    pub fn new(started_at: Duration) -> Self {
        Self {
            started_at,
            trackers: BTreeMap::new(),
            controllers: BTreeMap::new(),
        }
    }

    /// Adds a valid raw dongle pose. The caller filters tracker states.
    pub fn add_tracker(&mut self, id: u32, sample: TimedSample) {
        add_sample(&mut self.trackers, self.started_at, id, sample);
    }

    /// Adds a valid SteamVR controller pose. Both clocks use the same origin.
    pub fn add_controller(&mut self, id: u32, sample: TimedSample) {
        add_sample(&mut self.controllers, self.started_at, id, sample);
    }

    /// Reports the best coverage among tracker streams within this attempt.
    pub fn rotation_progress(&self, now: Duration, config: &CalibrationConfig) -> RotationProgress {
        let until = now.min(self.started_at.saturating_add(config.timeout));
        self.trackers
            .values()
            .map(|records| {
                rotation_progress(
                    through(records, until)
                        .iter()
                        .map(|sample| sample.pose.rotation),
                    config.min_motion_rad,
                    config.min_axis_separation_rad,
                )
            })
            .max()
            .unwrap_or_default()
    }

    /// Whether a controller has enough valid intervals to estimate correlation.
    pub fn has_controller_samples(&self, now: Duration, config: &CalibrationConfig) -> bool {
        let until = now.min(self.started_at.saturating_add(config.timeout));
        self.controllers.values().any(|records| {
            angular_speeds(through(records, until), config.max_sample_gap)
                .nth((MIN_CORRELATION_POINTS - 1) as usize)
                .is_some()
        })
    }

    pub(super) fn select(
        &self,
        now: Duration,
        config: &CalibrationConfig,
    ) -> Result<PairSelection, CalibrationError> {
        let deadline = self.started_at.saturating_add(config.timeout);
        let until = now.min(deadline);
        let trackers = streams(&self.trackers, until, config.max_sample_gap);
        let controllers = streams(&self.controllers, until, config.max_sample_gap);
        let more_motion = if now >= deadline {
            CalibrationError::TimedOut
        } else {
            CalibrationError::MoreRotation
        };
        let enough_rotation = |rotations: &[TimedSample]| {
            sufficient_motion(
                rotations.iter().map(|record| record.pose.rotation),
                config.min_motion_rad,
                config.min_axis_separation_rad,
            )
        };
        if !trackers.iter().any(|stream| enough_rotation(stream.poses)) {
            return Err(more_motion);
        }

        let best = best_match(&trackers, &controllers, config, more_motion)?;
        Ok(PairSelection {
            tracker: best.tracker.id,
            controller: best.controller.id,
            offset_ms: best.offset_ms,
            correlation: best.correlation,
            pairs: best.pairs(config.max_sample_gap).collect(),
        })
    }
}

fn add_sample(
    devices: &mut BTreeMap<u32, Vec<TimedSample>>,
    start: Duration,
    id: u32,
    mut sample: TimedSample,
) {
    let Vec3 { x, y, z } = sample.pose.position;
    let Some(rotation) = sample.pose.rotation.normalized() else {
        return;
    };
    if sample.at < start || ![x, y, z].into_iter().all(f64::is_finite) {
        return;
    }
    sample.pose.rotation = rotation;
    let stream = devices.entry(id).or_default();
    match stream.binary_search_by_key(&sample.at, |previous| previous.at) {
        Ok(index) => stream[index] = sample,
        Err(index) => stream.insert(index, sample),
    }
}

struct Stream<'a> {
    id: u32,
    poses: &'a [TimedSample],
    speeds: Vec<SpeedSample>,
}

fn through(records: &[TimedSample], until: Duration) -> &[TimedSample] {
    &records[..records.partition_point(|sample| sample.at <= until)]
}

fn streams(
    devices: &BTreeMap<u32, Vec<TimedSample>>,
    until: Duration,
    max_gap: Duration,
) -> Vec<Stream<'_>> {
    devices
        .iter()
        .map(|(&id, records)| {
            let poses = through(records, until);
            Stream {
                id,
                poses,
                speeds: angular_speeds(poses, max_gap).collect(),
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct SpeedSample {
    at: Duration,
    radians_per_second: f64,
}

fn angular_speeds(
    poses: &[TimedSample],
    max_gap: Duration,
) -> impl Iterator<Item = SpeedSample> + '_ {
    poses.windows(2).filter_map(move |window| {
        let first = window[0];
        let second = window[1];
        let gap = second.at - first.at;
        if gap.is_zero() || gap > max_gap {
            return None;
        }
        let delta = second.pose.rotation * first.pose.rotation.conjugate();
        let angle = 2.0
            * Vec3::new(delta.x, delta.y, delta.z)
                .norm()
                .atan2(delta.w.abs());
        // An interval-average rate corresponds to the interval midpoint.
        Some(SpeedSample {
            at: first.at + gap / 2,
            radians_per_second: angle / gap.as_secs_f64(),
        })
    })
}

struct Match<'a> {
    tracker: &'a Stream<'a>,
    controller: &'a Stream<'a>,
    offset_ms: i32,
    correlation: f64,
}

impl Match<'_> {
    fn pairs(&self, max_gap: Duration) -> impl Iterator<Item = SamplePair> + '_ {
        self.controller.poses.iter().filter_map(move |sample| {
            let at = tracker_time(sample.at, self.offset_ms)?;
            let tracker = pose_at(self.tracker.poses, at, max_gap)?;
            Some(SamplePair {
                tracker,
                controller: sample.pose,
            })
        })
    }

    fn tie_break(&self) -> (u32, i32, u32, u32) {
        (
            self.offset_ms.unsigned_abs(),
            self.offset_ms,
            self.tracker.id,
            self.controller.id,
        )
    }

    fn better_than(&self, previous: &Self) -> bool {
        self.correlation > previous.correlation
            || (self.correlation == previous.correlation && self.tie_break() < previous.tie_break())
    }
}

fn best_match<'a>(
    trackers: &'a [Stream<'a>],
    controllers: &'a [Stream<'a>],
    config: &CalibrationConfig,
    more_motion: CalibrationError,
) -> Result<Match<'a>, CalibrationError> {
    let mut best = None;
    let mut correlated = false;
    for tracker in trackers {
        for controller in controllers {
            for offset_ms in offsets(&tracker.speeds, &controller.speeds, config) {
                let Some(correlation) = correlation(
                    &tracker.speeds,
                    &controller.speeds,
                    offset_ms,
                    config.max_sample_gap,
                )
                .filter(|&score| score >= config.min_correlation) else {
                    continue;
                };
                correlated = true;
                let candidate = Match {
                    tracker,
                    controller,
                    offset_ms,
                    correlation,
                };
                // A short overlap can correlate without covering both axes.
                if best
                    .as_ref()
                    .is_none_or(|previous| candidate.better_than(previous))
                    && sufficient_motion(
                        candidate
                            .pairs(config.max_sample_gap)
                            .map(|pair| pair.tracker.rotation),
                        config.min_motion_rad,
                        config.min_axis_separation_rad,
                    )
                {
                    best = Some(candidate);
                }
            }
        }
    }
    best.ok_or(if correlated {
        more_motion
    } else {
        CalibrationError::Uncorrelated
    })
}

fn offsets(
    tracker: &[SpeedSample],
    controller: &[SpeedSample],
    config: &CalibrationConfig,
) -> impl Iterator<Item = i32> {
    let span = i64::from(config.max_offset_ms.max(0));
    let step = i64::from(config.offset_step_ms.max(1));
    // Skip offsets whose source times never overlap. Keep the configured grid.
    let grid_start = -span;
    let bounds = tracker
        .first()
        .zip(tracker.last())
        .zip(controller.first().zip(controller.last()));
    let (lower, upper) = bounds.map_or((1, 0), |((tf, tl), (cf, cl))| {
        let difference = |controller: Duration, tracker: Duration| {
            controller.as_nanos() as i128 - tracker.as_nanos() as i128
        };
        let earliest = difference(cf.at, tl.at);
        let latest = difference(cl.at, tf.at);
        let lower = (-(-earliest).div_euclid(1_000_000)).max(i128::from(grid_start));
        let upper = latest.div_euclid(1_000_000).min(i128::from(span));
        (
            i64::try_from(lower).unwrap_or(i64::MAX),
            i64::try_from(upper).unwrap_or(i64::MIN),
        )
    });
    let first = lower.saturating_add((grid_start - lower.rem_euclid(step)).rem_euclid(step));
    (first..=upper)
        .step_by(step as usize)
        .map(|offset| offset as i32)
}

fn tracker_time(controller_at: Duration, offset_ms: i32) -> Option<Duration> {
    let shift = Duration::from_millis(u64::from(offset_ms.unsigned_abs()));
    if offset_ms >= 0 {
        controller_at.checked_sub(shift)
    } else {
        controller_at.checked_add(shift)
    }
}

fn surrounding<T>(
    samples: &[T],
    at: Duration,
    max_gap: Duration,
    timestamp: impl Fn(&T) -> Duration,
) -> Option<(&T, &T, f64)> {
    let index = samples.partition_point(|sample| timestamp(sample) < at);
    let later = samples.get(index)?;
    let later_at = timestamp(later);
    if later_at == at {
        return Some((later, later, 0.0));
    }
    let earlier = samples.get(index.checked_sub(1)?)?;
    let earlier_at = timestamp(earlier);
    let gap = later_at - earlier_at;
    if gap > max_gap {
        return None;
    }
    Some((
        earlier,
        later,
        (at - earlier_at).as_secs_f64() / gap.as_secs_f64(),
    ))
}

fn pose_at(poses: &[TimedSample], at: Duration, max_gap: Duration) -> Option<Pose> {
    let (earlier, later, fraction) = surrounding(poses, at, max_gap, |sample| sample.at)?;
    if fraction == 0.0 {
        return Some(earlier.pose);
    }
    Some(Pose {
        position: earlier.pose.position * (1.0 - fraction) + later.pose.position * fraction,
        rotation: earlier.pose.rotation.slerp(later.pose.rotation, fraction)?,
    })
}

fn correlation(
    tracker: &[SpeedSample],
    controller: &[SpeedSample],
    offset_ms: i32,
    max_gap: Duration,
) -> Option<f64> {
    let mut count = 0_u32;
    let (mut mean_x, mut mean_y, mut var_x, mut var_y, mut cov) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for sample in controller {
        let Some(at) = tracker_time(sample.at, offset_ms) else {
            continue;
        };
        let Some((earlier, later, fraction)) = surrounding(tracker, at, max_gap, |row| row.at)
        else {
            continue;
        };
        let x = earlier.radians_per_second * (1.0 - fraction) + later.radians_per_second * fraction;
        let y = sample.radians_per_second;
        // Online centered covariance avoids cancellation for nearly constant rates.
        count += 1;
        let delta_x = x - mean_x;
        let delta_y = y - mean_y;
        mean_x += delta_x / f64::from(count);
        mean_y += delta_y / f64::from(count);
        var_x += delta_x * (x - mean_x);
        var_y += delta_y * (y - mean_y);
        cov += delta_x * (y - mean_y);
    }
    // At two samples, the magnitude of Pearson's correlation is always one.
    if count < MIN_CORRELATION_POINTS || var_x <= f64::EPSILON || var_y <= f64::EPSILON {
        return None;
    }
    Some((cov / (var_x.sqrt() * var_y.sqrt())).clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    use crate::Quat;

    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn sample(at: Duration, position: Vec3, rotation: Quat) -> TimedSample {
        TimedSample {
            at,
            pose: Pose { position, rotation },
        }
    }

    #[test]
    fn interpolation_uses_the_raw_poses_and_does_not_cross_a_large_gap() {
        let records = [
            sample(ms(10), Vec3::ZERO, Quat::IDENTITY),
            sample(ms(60), Vec3::new(2.0, 4.0, 6.0), Quat::yaw(2.0)),
            sample(ms(111), Vec3::ZERO, Quat::IDENTITY),
        ];
        let middle = pose_at(&records, ms(35), ms(50)).unwrap();
        assert_eq!(middle.position, Vec3::new(1.0, 2.0, 3.0));
        let yaw = Quat::yaw(1.0);
        assert!((middle.rotation.w - yaw.w).abs() < 1e-12);
        assert!((middle.rotation.y - yaw.y).abs() < 1e-12);
        assert!(pose_at(&records, ms(70), ms(50)).is_none());
        assert_eq!(pose_at(&records, ms(111), ms(50)), Some(records[2].pose));
        assert!(pose_at(&records, ms(9), ms(50)).is_none());
        assert!(pose_at(&records, ms(112), ms(50)).is_none());
    }

    #[test]
    fn pairing_finds_the_signed_offset_even_with_different_sampling_rates() {
        let mut collector = Collector::new(Duration::ZERO);
        for time in (0..6_000).step_by(10) {
            let t = ms(time).as_secs_f64();
            let (sin_pitch, cos_pitch) = (0.65 * (2.8 * t).sin()).sin_cos();
            let (sin_roll, cos_roll) = (0.75 * (3.9 * t).sin()).sin_cos();
            let rotation = Quat::yaw((6.1 * t).sin())
                * Quat::new(cos_pitch, sin_pitch, 0.0, 0.0)
                * Quat::new(cos_roll, 0.0, 0.0, sin_roll);
            let position = Vec3::new(t, 0.0, 0.0);
            collector.add_tracker(3, sample(ms(time + 35), position, rotation));
            if time % 30 == 0 {
                collector.add_controller(4, sample(ms(time), position, rotation));
                collector.add_controller(5, sample(ms(time), position, Quat::IDENTITY));
            }
        }
        let selected = collector
            .select(ms(6_100), &CalibrationConfig::default())
            .unwrap();
        assert_eq!((selected.tracker, selected.controller), (3, 4));
        assert_eq!(selected.offset_ms, -35);
        assert!(selected.correlation > 0.99);
        assert!(selected.pairs.len() > 190);
    }

    #[test]
    fn insufficient_motion_times_out_at_the_supplied_deadline() {
        let mut collector = Collector::new(ms(1_000));
        collector.add_tracker(0, sample(ms(1_000), Vec3::ZERO, Quat::IDENTITY));
        collector.add_tracker(
            0,
            sample(ms(1_033), Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY),
        );
        // These rotations arrive too late to make this attempt succeed.
        collector.add_tracker(0, sample(ms(21_001), Vec3::ZERO, Quat::yaw(1.1)));
        collector.add_tracker(
            0,
            sample(ms(21_034), Vec3::ZERO, Quat::new(0.5, 0.0, 0.0, 0.5)),
        );
        let config = CalibrationConfig::default();
        assert!(matches!(
            collector.select(ms(20_999), &config),
            Err(CalibrationError::MoreRotation)
        ));
        assert!(matches!(
            collector.select(ms(21_000), &config),
            Err(CalibrationError::TimedOut)
        ));
    }

    #[test]
    fn samples_before_the_start_are_ignored_and_equal_times_keep_the_last_valid_pose() {
        let mut collector = Collector::new(ms(10));
        let invalid = Quat::new(f64::NAN, 0.0, 0.0, 0.0);
        collector.add_tracker(0, sample(ms(9), Vec3::ZERO, Quat::IDENTITY));
        collector.add_tracker(0, sample(ms(25), Vec3::ZERO, Quat::IDENTITY));
        collector.add_tracker(0, sample(ms(10), Vec3::ZERO, Quat::IDENTITY));
        collector.add_tracker(0, sample(ms(25), Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY));
        collector.add_tracker(0, sample(ms(25), Vec3::ZERO, invalid));
        let records = &collector.trackers[&0];
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].at, ms(10));
        assert_eq!(records[1].at, ms(25));
        assert_eq!(records[1].pose.position.x, 1.0);
    }
}
