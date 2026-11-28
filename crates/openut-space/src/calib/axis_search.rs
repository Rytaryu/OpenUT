use crate::AxisMap;

use super::solve::{self, Candidate};
use super::{CalibrationConfig, CalibrationError, SamplePair};

pub(super) struct Fit {
    pub axis_map: AxisMap,
    pub candidate: Candidate,
    pub residual_ratio: Option<f64>,
}

pub(super) fn search(
    samples: &[SamplePair],
    config: &CalibrationConfig,
) -> Result<Fit, CalibrationError> {
    let mut candidates = Vec::new();
    let mut solved = false;
    for axis_map in AxisMap::all().filter(|axis| *axis == axis.group().representative()) {
        let Ok(candidate) = solve::solve(samples, axis_map, config) else {
            continue;
        };
        solved = true;
        if candidate.calibration.scale > 0.0 && candidate.position_rms_m.is_finite() {
            candidates.push((axis_map, candidate));
        }
    }
    if !solved {
        return Err(CalibrationError::MoreRotation);
    }
    candidates.sort_by(|(map_a, a), (map_b, b)| {
        a.position_rms_m
            .total_cmp(&b.position_rms_m)
            .then_with(|| map_a.cmp(map_b))
    });
    let mut candidates = candidates.into_iter();
    let (axis_map, candidate) = candidates.next().ok_or(CalibrationError::AxisMismatch)?;
    let residual_ratio = candidates.next().map(|(_, runner_up)| {
        if runner_up.position_rms_m == candidate.position_rms_m {
            1.0 // Includes two exact fits.
        } else {
            runner_up.position_rms_m / candidate.position_rms_m
        }
    });
    if residual_ratio.is_some_and(|ratio| ratio < config.min_axis_residual_ratio) {
        return Err(CalibrationError::AxisMismatch);
    }

    if candidate.position_rms_m > config.max_position_rms_m
        || candidate.orientation_spread_rad > config.max_orientation_spread_rad
    {
        return Err(CalibrationError::GripSlipped);
    }
    if candidate.vertical_axis_error > config.max_vertical_axis_error {
        return Err(CalibrationError::AxisMismatch);
    }
    if !(config.min_scale..=config.max_scale).contains(&candidate.calibration.scale) {
        return Err(CalibrationError::ScaleOutOfRange);
    }

    Ok(Fit {
        axis_map,
        candidate,
        residual_ratio,
    })
}
