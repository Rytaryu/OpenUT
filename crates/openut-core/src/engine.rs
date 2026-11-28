//! Drives tracker state, detection, and notices from ordered log records without I/O.

use std::collections::BTreeMap;

use crate::event::{Event, Timed};
use crate::notice::{Notice, NoticeConfig, NoticeScheduler};
use crate::rules::{Detection, Rules, RulesConfig};
use crate::state::{DEFAULT_DUP_MS, Snapshot, State};
use crate::time::{LogDate, LogTime, UtcOffset};

#[cfg(test)]
mod notices_tests;
#[cfg(test)]
mod tests;

const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineConfig {
    pub rules: RulesConfig,
    pub state: StateConfig,
    pub notice: NoticeConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateConfig {
    pub dup_ms: i64,
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            dup_ms: DEFAULT_DUP_MS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    Server,
    Driver,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineQuality {
    Other,
    Unparsed,
    NoHeader,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineCounts {
    pub lines: u64,
    pub other: u64,
    pub unparsed: u64,
    pub no_header: u64,
}

impl LineCounts {
    fn record(&mut self, quality: Option<LineQuality>) {
        self.lines = self.lines.saturating_add(1);
        let count = match quality {
            Some(LineQuality::Other) => &mut self.other,
            Some(LineQuality::Unparsed) => &mut self.unparsed,
            Some(LineQuality::NoHeader) => &mut self.no_header,
            None => return,
        };
        *count = count.saturating_add(1);
    }

    fn merge(&mut self, other: Self) {
        self.lines = self.lines.saturating_add(other.lines);
        self.other = self.other.saturating_add(other.other);
        self.unparsed = self.unparsed.saturating_add(other.unparsed);
        self.no_header = self.no_header.saturating_add(other.no_header);
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineStats {
    pub time_regressions: u64,
    pub power_offs: u64,
    /// Notification deliveries committed by `Engine::drain`.
    pub pushes: u64,
    pub server: LineCounts,
    pub driver: LineCounts,
    pub driver_days: BTreeMap<LogDate, LineCounts>,
}

impl EngineStats {
    /// Accumulates counts from separate replay segments.
    pub fn merge(&mut self, other: Self) {
        self.time_regressions = self.time_regressions.saturating_add(other.time_regressions);
        self.power_offs = self.power_offs.saturating_add(other.power_offs);
        self.pushes = self.pushes.saturating_add(other.pushes);
        self.server.merge(other.server);
        self.driver.merge(other.driver);
        for (day, counts) in other.driver_days {
            self.driver_days.entry(day).or_default().merge(counts);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Output {
    Detection(Detection),
    Notice(Notice),
}

pub struct Engine {
    cfg: EngineConfig,
    state: State,
    rules: Rules,
    now: Option<LogTime>,
    offset: UtcOffset,
    detections: Vec<Detection>,
    notices: NoticeScheduler,
    stats: EngineStats,
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> Self {
        Self {
            state: State::new(cfg.state.dup_ms),
            notices: NoticeScheduler::new(cfg.notice.clone(), cfg.rules.r5.wait_s),
            cfg,
            rules: Rules::default(),
            now: None,
            offset: UtcOffset(0),
            detections: Vec::new(),
            stats: EngineStats::default(),
        }
    }

    pub fn now(&self) -> Option<LogTime> {
        self.now
    }

    /// Runs deadlines strictly before `at`. Server records at `at` can still cancel
    /// or update timers scheduled for the same instant.
    pub fn advance_to(&mut self, at: LogTime) {
        if self.now.is_some_and(|now| now >= at) {
            return;
        }
        let first = self.detections.len();
        let power_offs = self
            .rules
            .advance(at, &self.state, &self.cfg.rules, &mut self.detections);
        self.stats.power_offs = self.stats.power_offs.saturating_add(power_offs);
        self.notice_detections(first);
        self.notices.advance_to(at);
        self.now = Some(at);
    }

    pub fn apply(&mut self, mut timed: Timed) {
        if matches!(timed.event, Event::PowerOffNotice { .. }) {
            self.record_driver_line(Some((timed.at, timed.offset)), None);
            let first = self.detections.len();
            self.rules
                .remember_driver_event(&timed, &self.cfg.rules, &mut self.detections);
            self.notice_detections(first);
            return;
        }
        self.stats.server.record(None);
        timed.at = self.server_time(timed.at, timed.offset);
        let update = self.state.apply(&timed);
        let first = self.detections.len();
        self.rules.apply(
            &timed,
            &update.connections,
            &self.state,
            &self.cfg.rules,
            &mut self.detections,
        );
        self.notice_detections(first);
    }

    pub fn note_line(&mut self, file: FileKind, at: Option<LogTime>, quality: LineQuality) {
        self.note_line_at(file, at.map(|at| (at, self.offset)), quality);
    }

    /// Records the offset of a line with a parsed header, including a driver's
    /// records outside the server's current day.
    pub fn note_line_at(
        &mut self,
        file: FileKind,
        at: Option<(LogTime, UtcOffset)>,
        quality: LineQuality,
    ) {
        match file {
            FileKind::Server => {
                self.stats.server.record(Some(quality));
                if let Some((at, offset)) = at {
                    self.server_time(at, offset);
                }
            }
            FileKind::Driver => self.record_driver_line(at, Some(quality)),
        }
    }

    /// Driver records through this inclusive instant must arrive before server
    /// time can move beyond it.
    pub fn driver_needed_through(&self) -> Option<LogTime> {
        let now = self.now?;
        let local_ms =
            (now.0.rem_euclid(DAY_MS) + i64::from(self.offset.0) * 60_000).rem_euclid(DAY_MS);
        let day_end = LogTime(now.0.saturating_add(DAY_MS - 1 - local_ms));
        if self.cfg.rules.r4.exclude_power_off
            && let Some(deadline) = self.rules.r4_deadline()
        {
            return Some(deadline.min(day_end));
        }
        Some(day_end)
    }

    pub fn next_deadline(&self) -> Option<LogTime> {
        self.rules
            .next_deadline()
            .into_iter()
            .chain(self.notices.next_deadline())
            .min()
    }

    pub fn set_catching_up(&mut self, on: bool) {
        self.notices
            .set_catching_up(on, self.now.unwrap_or(LogTime(0)));
    }

    /// Suppresses delivery while the app considers Hub stopped or starting.
    pub fn set_hub_down(&mut self, on: bool) {
        self.notices.set_hub_down(on);
    }

    pub fn rebuild(&mut self) {
        self.state = State::new(self.cfg.state.dup_ms);
        self.rules = Rules::default();
        self.now = None;
        self.offset = UtcOffset(0);
        self.detections.clear();
        self.notices.rebuild();
        self.stats = EngineStats {
            pushes: self.stats.pushes,
            ..EngineStats::default()
        };
    }

    pub fn drain(&mut self) -> Vec<Output> {
        let notices = self.notices.drain();
        let pushes = notices.iter().filter(|notice| notice.push).count() as u64;
        self.stats.pushes = self.stats.pushes.saturating_add(pushes);
        self.detections
            .drain(..)
            .map(Output::Detection)
            .chain(notices.into_iter().map(Output::Notice))
            .collect()
    }

    pub fn live_notices(&self) -> Vec<Notice> {
        self.notices.live()
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot(self.now.unwrap_or(LogTime(0)))
    }

    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }

    fn notice_detections(&mut self, first: usize) {
        for detection in &self.detections[first..] {
            self.notices.advance_to(detection.at);
            let snapshot = self.state.snapshot(detection.at);
            self.notices.apply(detection, &snapshot);
        }
    }

    fn server_time(&mut self, at: LogTime, offset: UtcOffset) -> LogTime {
        if let Some(now) = self.now.filter(|now| at < *now) {
            self.stats.time_regressions = self.stats.time_regressions.saturating_add(1);
            return now;
        }
        self.advance_to(at);
        self.offset = offset;
        at
    }

    fn record_driver_line(
        &mut self,
        at: Option<(LogTime, UtcOffset)>,
        quality: Option<LineQuality>,
    ) {
        self.stats.driver.record(quality);
        if let Some((at, offset)) = at {
            let (day, _) = at.to_local(offset);
            self.stats
                .driver_days
                .entry(day)
                .or_default()
                .record(quality);
        }
    }
}
