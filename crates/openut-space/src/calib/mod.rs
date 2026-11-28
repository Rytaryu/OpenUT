//! Calibration of raw dongle poses against SteamVR controller poses.

mod axis_search;
mod collect;
mod solve;

use std::time::Duration;

use crate::{AxisMap, Calibration};

use collect::SamplePair;

pub use collect::{Collector, TimedSample};

/// Tunable collection and fit limits from the walkthrough, with angles in radians.
#[derive(Clone, Debug, PartialEq)]
pub struct CalibrationConfig {
    pub sample_hz: u32,
    pub timeout: Duration,
    pub max_offset_ms: i32,
    pub offset_step_ms: i32,
    pub min_correlation: f64,
    pub max_sample_gap: Duration,
    pub min_motion_rad: f64,
    pub min_axis_separation_rad: f64,
    pub min_pair_rotation_rad: f64,
    pub min_pivot_ratio: f64,
    pub max_position_rms_m: f64,
    pub max_orientation_spread_rad: f64,
    pub max_vertical_axis_error: f64,
    pub min_axis_residual_ratio: f64,
    pub min_scale: f64,
    pub max_scale: f64,
    pub fix_scale: bool,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            sample_hz: 30,
            timeout: Duration::from_secs(20),
            max_offset_ms: 150,
            offset_step_ms: 5,
            min_correlation: 0.8,
            max_sample_gap: Duration::from_millis(50),
            min_motion_rad: 30.0_f64.to_radians(),
            min_axis_separation_rad: 45.0_f64.to_radians(),
            min_pair_rotation_rad: 15.0_f64.to_radians(),
            min_pivot_ratio: 1e-6,
            max_position_rms_m: 0.02,
            max_orientation_spread_rad: 3.0_f64.to_radians(),
            max_vertical_axis_error: 0.1,
            min_axis_residual_ratio: 1.5,
            min_scale: 0.9,
            max_scale: 1.1,
            fix_scale: false,
        }
    }
}

/// Outcomes the caller can use to choose how to continue a calibration attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalibrationError {
    MoreRotation,
    Uncorrelated,
    GripSlipped,
    AxisMismatch,
    ScaleOutOfRange,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationResult {
    /// Replaces the configured axis map and is applied directly to raw dongle poses.
    pub axis_map: AxisMap,
    /// Applied after `axis_map` to position and orientation.
    pub calibration: Calibration,
    pub tracker: u32,
    pub controller: u32,
    /// The tracker pose was interpolated at controller time minus this offset.
    pub offset_ms: i32,
    pub correlation: f64,
    pub sample_count: usize,
    pub position_rms_m: f64,
    pub orientation_spread_rad: f64,
    pub vertical_axis_error: f64,
    /// Runner-up RMS divided by the winner's RMS, if there was a runner-up.
    pub axis_residual_ratio: Option<f64>,
}

impl Collector {
    /// Selects a device pair and fits the raw poses using the supplied clock value.
    pub fn calibrate(
        &self,
        now: Duration,
        config: &CalibrationConfig,
    ) -> Result<CalibrationResult, CalibrationError> {
        let selection = self.select(now, config)?;
        let fit = axis_search::search(&selection.pairs, config)?;
        Ok(CalibrationResult {
            axis_map: fit.axis_map,
            calibration: fit.candidate.calibration,
            tracker: selection.tracker,
            controller: selection.controller,
            offset_ms: selection.offset_ms,
            correlation: selection.correlation,
            sample_count: selection.pairs.len(),
            position_rms_m: fit.candidate.position_rms_m,
            orientation_spread_rad: fit.candidate.orientation_spread_rad,
            vertical_axis_error: fit.candidate.vertical_axis_error,
            axis_residual_ratio: fit.residual_ratio,
        })
    }
}
