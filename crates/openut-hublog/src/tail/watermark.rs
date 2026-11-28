//! Estimate stream progress from record times and the wall clock (watcher §4.6).

use openut_core::time::LogTime;

const OFFSET_WINDOW_MS: i64 = 5 * 60 * 1_000;
const DEFAULT_LATENESS_MS: i64 = 10_000;

#[derive(Debug)]
struct Sample {
    wall: LogTime,
    offset: i64,
}

/// Server and driver watermarks sharing a wall-clock offset measured on the server.
/// Start a fresh instance when the log streams are rebuilt.
#[derive(Debug)]
pub struct Watermark {
    lateness_ms: i64,
    last_server: Option<LogTime>,
    last_driver: Option<LogTime>,
    read_complete: bool,
    samples: Vec<Sample>,
    last_offset: Option<i64>,
}

impl Default for Watermark {
    fn default() -> Self {
        Self::new(DEFAULT_LATENESS_MS)
    }
}

impl Watermark {
    pub fn new(lateness_ms: i64) -> Self {
        Self {
            lateness_ms: lateness_ms.max(0),
            last_server: None,
            last_driver: None,
            read_complete: false,
            samples: Vec::new(),
            last_offset: None,
        }
    }

    /// Adopt the last server entry in a read batch, even if its time regressed.
    pub fn observe_server_batch(&mut self, now: LogTime, last: LogTime) {
        self.last_server = Some(last);
        if self.read_complete {
            self.measure_offset(now, last);
        }
    }

    /// Adopt the last driver entry without measuring a wall-clock offset.
    pub fn observe_driver_batch(&mut self, last: LogTime) {
        self.last_driver = Some(last);
    }

    pub(super) fn clear_driver(&mut self) {
        self.last_driver = None;
    }

    pub(super) fn driver_logged_time(&self) -> Option<LogTime> {
        self.last_driver
    }

    /// Mark both initial reads finished. Measure d₀ once, even if delivery is still gated.
    pub fn mark_read_complete(&mut self, now: LogTime) {
        if !std::mem::replace(&mut self.read_complete, true)
            && let Some(last) = self.last_server
        {
            self.measure_offset(now, last);
        }
    }

    /// The least offset in the last five minutes, or the last measurement.
    pub fn offset(&self, now: LogTime) -> Option<i64> {
        self.samples
            .iter()
            .filter(|sample| in_window(sample.wall, now))
            .map(|sample| sample.offset)
            .min()
            .or(self.last_offset)
    }

    pub fn server_time(&self, now: LogTime) -> Option<LogTime> {
        self.last_server.map(|last| {
            self.time_from_wall(now)
                .map_or(last, |estimate| last.max(estimate))
        })
    }

    /// Before both initial reads finish, only observed driver times count.
    /// A missing driver file bypasses the deadline gate at the caller.
    pub fn driver_time(&self, now: LogTime) -> Option<LogTime> {
        self.last_driver.max(self.time_from_wall(now))
    }

    fn measure_offset(&mut self, now: LogTime, last: LogTime) {
        let offset = now.0.saturating_sub(last.0);
        self.last_offset = Some(offset);
        self.samples.retain(|sample| in_window(sample.wall, now));
        if let Some(sample) = self.samples.last_mut().filter(|sample| sample.wall == now) {
            // Repeated reads may share a wall-clock tick. The window needs only its minimum.
            sample.offset = sample.offset.min(offset);
        } else {
            self.samples.push(Sample { wall: now, offset });
        }
    }

    fn time_from_wall(&self, now: LogTime) -> Option<LogTime> {
        let offset = self.offset(now)?;
        let at = i128::from(now.0) - i128::from(offset) - i128::from(self.lateness_ms);
        Some(LogTime(
            at.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64,
        ))
    }
}

fn in_window(wall: LogTime, now: LogTime) -> bool {
    (LogTime(now.0.saturating_sub(OFFSET_WINDOW_MS))..=now).contains(&wall)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_up_uses_observed_records_until_read_complete() {
        let mut mark = Watermark::default();
        let late_wall = LogTime(48_000);
        assert_eq!(mark.server_time(late_wall), None);
        assert_eq!(mark.driver_time(late_wall), None);
        mark.observe_server_batch(LogTime(27_000), LogTime(100));
        mark.observe_server_batch(LogTime(28_000), LogTime(10_000));
        mark.observe_driver_batch(LogTime(9_600));
        assert_eq!(mark.offset(late_wall), None);
        assert_eq!(mark.server_time(late_wall), Some(LogTime(10_000)));
        assert_eq!(mark.driver_time(late_wall), Some(LogTime(9_600)));
    }

    #[test]
    fn d0_allows_both_streams_to_advance_when_the_files_stay_idle() {
        let mut mark = Watermark::default();
        mark.observe_server_batch(LogTime(5_300), LogTime(5_100));
        mark.observe_driver_batch(LogTime(-3_000));
        mark.mark_read_complete(LogTime(5_300));
        assert_eq!(mark.offset(LogTime(5_300)), Some(200));
        assert_eq!(mark.server_time(LogTime(5_300)), Some(LogTime(5_100)));
        assert_eq!(mark.driver_time(LogTime(5_300)), Some(LogTime(-3_000)));
        let deadline = LogTime(5_000);
        assert_eq!(mark.driver_time(LogTime(15_200)), Some(deadline));
        assert_eq!(mark.driver_time(LogTime(15_201)), Some(LogTime(5_001)));
        assert_eq!(mark.server_time(LogTime(15_400)), Some(LogTime(5_200)));

        mark.mark_read_complete(LogTime(18_000));
        assert_eq!(mark.offset(LogTime(18_000)), Some(200));
        assert_eq!(mark.driver_time(LogTime(18_000)), Some(LogTime(7_800)));
    }

    #[test]
    fn uses_recent_server_minimum_and_falls_back_to_the_last_measurement() {
        let mut mark = Watermark::default();
        mark.observe_server_batch(LogTime(50_000), LogTime(36_000));
        mark.mark_read_complete(LogTime(50_000));
        assert_eq!(mark.offset(LogTime(50_000)), Some(14_000));

        mark.observe_server_batch(LogTime(51_000), LogTime(50_000));
        mark.observe_server_batch(LogTime(72_000), LogTime(69_000));
        mark.observe_driver_batch(LogTime(100_000));
        mark.observe_server_batch(LogTime(73_000), LogTime(66_000));
        assert_eq!(mark.offset(LogTime(73_000)), Some(1_000));
        assert_eq!(mark.driver_time(LogTime(73_000)), Some(LogTime(100_000)));
        assert_eq!(mark.offset(LogTime(351_000)), Some(1_000));
        assert_eq!(mark.offset(LogTime(351_001)), Some(3_000));
        assert_eq!(mark.offset(LogTime(372_001)), Some(7_000));
        assert_eq!(mark.offset(LogTime(400_000)), Some(7_000));
    }

    #[test]
    fn waiting_works_when_the_driver_file_has_no_timestamped_records() {
        let mut mark = Watermark::default();
        mark.mark_read_complete(LogTime(10_000));
        assert_eq!(mark.driver_time(LogTime(20_000)), None);
        assert_eq!(mark.server_time(LogTime(20_000)), None);
        mark.observe_server_batch(LogTime(21_000), LogTime(20_000));
        assert_eq!(mark.offset(LogTime(21_000)), Some(1_000));
        assert_eq!(mark.driver_time(LogTime(31_001)), Some(LogTime(20_001)));
    }

    #[test]
    fn retains_the_latest_measurement_for_fallback_even_on_the_same_tick() {
        let mut mark = Watermark::default();
        mark.mark_read_complete(LogTime(0));
        mark.observe_server_batch(LogTime(1_000), LogTime(1_200));
        mark.observe_server_batch(LogTime(1_000), LogTime(850));
        assert_eq!(mark.offset(LogTime(1_000)), Some(-200));
        assert_eq!(mark.offset(LogTime(301_001)), Some(150));
    }
}
