//! Calibration collection, fitting, and presentation state without I/O.

use std::time::Duration;

use openut_space::calib::{CalibrationConfig, CalibrationError, Collector, TimedSample};
use openut_space::fingerprint::{SpaceChange, SpaceFingerprint};
use openut_space::{AxisMap, Calibration};

pub use openut_space::calib::RotationProgress;

const FIT_RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// A calibration and the raw-space axis map it was fitted with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationRecord {
    pub axis_map: AxisMap,
    pub calibration: Calibration,
    pub position_rms_m: f64,
    pub sample_count: usize,
    pub offset_ms: i32,
    pub created_at_unix_s: u64,
    pub fingerprint: SpaceFingerprint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalibrationFailure {
    Fit(CalibrationError),
    SpaceUnavailable,
    SpaceChanged,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalibrationStatus {
    Needed,
    Calibrated,
    Collecting {
        motion: RotationProgress,
    },
    Completed {
        position_rms_m: f64,
        sample_count: usize,
    },
    Failed(CalibrationFailure),
    NeedsRecalibration,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationView {
    pub status: CalibrationStatus,
    pub has_calibration: bool,
    pub needs_recalibration: bool,
}

struct Attempt {
    collector: Collector,
    fingerprint: SpaceFingerprint,
    deadline: Duration,
    next_fit: Duration,
    motion: RotationProgress,
}

enum State {
    Idle,
    Collecting(Attempt),
    Completed,
    Failed(CalibrationFailure),
}

pub struct CalibrationFlow {
    config: CalibrationConfig,
    configured_axis: AxisMap,
    record: Option<CalibrationRecord>,
    fingerprint: Option<SpaceFingerprint>,
    needs_recalibration: bool,
    state: State,
}

impl CalibrationFlow {
    pub fn new(
        config: CalibrationConfig,
        configured_axis: AxisMap,
        saved: Option<CalibrationRecord>,
    ) -> Self {
        Self {
            config,
            configured_axis,
            record: saved,
            fingerprint: None,
            needs_recalibration: false,
            state: State::Idle,
        }
    }

    /// Returns the entire transform currently applied to raw dongle poses.
    pub fn active(&self) -> (AxisMap, Calibration) {
        self.record
            .map_or((self.configured_axis, Calibration::IDENTITY), |record| {
                (record.axis_map, record.calibration)
            })
    }

    pub fn is_collecting(&self) -> bool {
        matches!(self.state, State::Collecting(_))
    }

    /// Starts collecting without replacing an active calibration.
    pub fn start(&mut self, now: Duration) -> bool {
        if self.is_collecting() {
            return false;
        }
        let Some(fingerprint) = self.fingerprint else {
            self.state = State::Failed(CalibrationFailure::SpaceUnavailable);
            return false;
        };
        self.state = State::Collecting(Attempt {
            collector: Collector::new(now),
            fingerprint,
            deadline: now.saturating_add(self.config.timeout),
            next_fit: now,
            motion: RotationProgress::None,
        });
        true
    }

    pub fn cancel(&mut self) {
        if self.is_collecting() {
            self.state = State::Idle;
        }
    }

    /// Supplies current space evidence. Pass `None` on disconnect or when the
    /// SteamVR connection changes to avoid carrying samples between sessions.
    pub fn set_fingerprint(&mut self, fingerprint: Option<SpaceFingerprint>) {
        self.fingerprint = fingerprint;
        if let Some(current) = fingerprint {
            self.needs_recalibration = self
                .record
                .is_some_and(|record| record.fingerprint.compare(&current) == SpaceChange::Changed);
        }
        if let State::Collecting(attempt) = &self.state {
            match fingerprint {
                None => self.state = State::Failed(CalibrationFailure::SpaceUnavailable),
                Some(current) if attempt.fingerprint.compare(&current) == SpaceChange::Changed => {
                    self.state = State::Failed(CalibrationFailure::SpaceChanged);
                }
                Some(_) => {}
            }
        }
    }

    pub fn add_tracker(&mut self, device: u32, sample: TimedSample) {
        if let State::Collecting(attempt) = &mut self.state
            && sample.at <= attempt.deadline
        {
            attempt.collector.add_tracker(device, sample);
        }
    }

    pub fn add_controller(&mut self, device: u32, sample: TimedSample) {
        if let State::Collecting(attempt) = &mut self.state
            && sample.at <= attempt.deadline
        {
            attempt.collector.add_controller(device, sample);
        }
    }

    /// Applies and returns a new record once when the fit succeeds.
    pub fn tick(&mut self, now: Duration, unix_s: u64) -> Option<CalibrationRecord> {
        let State::Collecting(attempt) = &mut self.state else {
            return None;
        };
        let expired = now >= attempt.deadline;
        attempt.motion = attempt.collector.rotation_progress(now, &self.config);
        if attempt.motion != RotationProgress::Ready
            || !attempt.collector.has_controller_samples(now, &self.config)
        {
            if expired {
                self.state = State::Failed(CalibrationFailure::Fit(CalibrationError::TimedOut));
            }
            return None;
        }
        if !expired && now < attempt.next_fit {
            return None;
        }
        attempt.next_fit = now.saturating_add(FIT_RETRY_INTERVAL);
        let fit = match attempt.collector.calibrate(now, &self.config) {
            Ok(fit) => fit,
            Err(CalibrationError::MoreRotation) if !expired => return None,
            Err(error) => {
                self.state = State::Failed(CalibrationFailure::Fit(error));
                return None;
            }
        };
        let Some(fingerprint) = self.fingerprint else {
            self.state = State::Failed(CalibrationFailure::SpaceUnavailable);
            return None;
        };
        let record = CalibrationRecord {
            axis_map: fit.axis_map,
            calibration: fit.calibration,
            position_rms_m: fit.position_rms_m,
            sample_count: fit.sample_count,
            offset_ms: fit.offset_ms,
            created_at_unix_s: unix_s,
            fingerprint,
        };
        self.record = Some(record);
        self.needs_recalibration = false;
        self.state = State::Completed;
        Some(record)
    }

    pub fn view(&self) -> CalibrationView {
        let status = match (&self.state, self.record) {
            (State::Collecting(attempt), _) => CalibrationStatus::Collecting {
                motion: attempt.motion,
            },
            (State::Failed(failure), _) => CalibrationStatus::Failed(*failure),
            (_, _) if self.needs_recalibration => CalibrationStatus::NeedsRecalibration,
            (State::Completed, Some(record)) => CalibrationStatus::Completed {
                position_rms_m: record.position_rms_m,
                sample_count: record.sample_count,
            },
            (_, Some(_)) => CalibrationStatus::Calibrated,
            (_, None) => CalibrationStatus::Needed,
        };
        CalibrationView {
            status,
            has_calibration: self.record.is_some(),
            needs_recalibration: self.needs_recalibration,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_6;

    use openut_space::{Pose, Quat, Vec3};

    use super::*;

    fn fingerprint(id: u64) -> SpaceFingerprint {
        SpaceFingerprint::new(
            id,
            Pose {
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            },
            None,
        )
        .unwrap()
    }

    fn raw_sample(ms: u64, rotation: Quat) -> TimedSample {
        TimedSample {
            at: Duration::from_millis(ms),
            pose: Pose {
                position: Vec3::ZERO,
                rotation,
            },
        }
    }

    #[test]
    fn deadlines_and_progress_use_the_current_trials_clipped_tracker_stream() {
        let mut flow = CalibrationFlow::new(CalibrationConfig::default(), AxisMap::IDENTITY, None);
        assert_eq!(flow.view().status, CalibrationStatus::Needed);
        assert!(!flow.start(Duration::ZERO));
        assert_eq!(
            flow.view().status,
            CalibrationStatus::Failed(CalibrationFailure::SpaceUnavailable)
        );

        flow.set_fingerprint(Some(fingerprint(1)));
        let start = Duration::from_secs(1);
        assert!(flow.start(start));
        assert!(!flow.start(Duration::from_secs(2)));

        let turn = FRAC_PI_6;
        let pitch = Quat::new(turn.cos(), turn.sin(), 0.0, 0.0);
        flow.add_tracker(1, raw_sample(1_000, Quat::IDENTITY));
        flow.add_tracker(1, raw_sample(1_200, Quat::yaw(1.0)));
        flow.add_tracker(2, raw_sample(1_000, Quat::IDENTITY));
        flow.add_tracker(2, raw_sample(1_200, pitch));
        flow.add_tracker(1, raw_sample(21_001, pitch));
        assert!(flow.tick(Duration::from_millis(20_999), 0).is_none());
        assert_eq!(
            flow.view().status,
            CalibrationStatus::Collecting {
                motion: RotationProgress::OneAxis
            }
        );

        flow.add_tracker(1, raw_sample(20_999, pitch));
        assert!(flow.tick(Duration::from_millis(20_999), 0).is_none());
        assert_eq!(
            flow.view().status,
            CalibrationStatus::Collecting {
                motion: RotationProgress::Ready
            }
        );
        assert!(flow.tick(Duration::from_secs(21), 0).is_none());
        assert_eq!(
            flow.view().status,
            CalibrationStatus::Failed(CalibrationFailure::Fit(CalibrationError::TimedOut))
        );
        assert_eq!(flow.active(), (AxisMap::IDENTITY, Calibration::IDENTITY));
        assert!(!flow.view().has_calibration);
    }

    #[test]
    fn space_change_and_cancel_keep_the_saved_axis_and_transform_together() {
        let old = fingerprint(1);
        let current = fingerprint(2);
        let saved = CalibrationRecord {
            axis_map: "+x+z-y".parse().unwrap(),
            calibration: Calibration::new(0.5, Vec3::new(1.0, 2.0, 3.0), 1.03),
            position_rms_m: 0.01,
            sample_count: 45,
            offset_ms: -20,
            created_at_unix_s: 0,
            fingerprint: old,
        };
        let mut flow =
            CalibrationFlow::new(CalibrationConfig::default(), AxisMap::IDENTITY, Some(saved));
        assert_eq!(flow.view().status, CalibrationStatus::Calibrated);
        flow.set_fingerprint(Some(current));
        assert_eq!(flow.view().status, CalibrationStatus::NeedsRecalibration);
        assert!(flow.start(Duration::ZERO));
        assert!(flow.view().needs_recalibration);
        flow.cancel();
        assert_eq!(flow.view().status, CalibrationStatus::NeedsRecalibration);

        assert!(flow.start(Duration::from_secs(1)));
        flow.set_fingerprint(Some(old));
        assert_eq!(
            flow.view().status,
            CalibrationStatus::Failed(CalibrationFailure::SpaceChanged)
        );
        assert!(!flow.view().needs_recalibration);
        assert!(flow.start(Duration::from_secs(2)));
        flow.set_fingerprint(None);
        assert_eq!(
            flow.view().status,
            CalibrationStatus::Failed(CalibrationFailure::SpaceUnavailable)
        );
        assert!(flow.tick(Duration::from_secs(3), 0).is_none());
        assert_eq!(flow.active(), (saved.axis_map, saved.calibration));
    }
}
