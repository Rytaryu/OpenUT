//! Drives tracker state, detection, and notices from ordered log records without I/O.

use std::collections::BTreeMap;

use crate::event::{Event, Timed};
use crate::notice::{Notice, NoticeConfig, NoticeScheduler};
use crate::rules::{Detection, Rules, RulesConfig};
use crate::state::{DEFAULT_DUP_MS, Snapshot, State};
use crate::summary::{Summaries, Summary, SummaryConfig, SummarySource};
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
    pub summary: SummaryConfig,
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
    pub long_losses: u64,
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
        self.long_losses = self.long_losses.saturating_add(other.long_losses);
        self.pushes = self.pushes.saturating_add(other.pushes);
        self.server.merge(other.server);
        self.driver.merge(other.driver);
        for (day, counts) in other.driver_days {
            self.driver_days.entry(day).or_default().merge(counts);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Detection(Detection),
    Notice(Notice),
    DayClosed(Box<Summary>),
}

pub struct Engine {
    cfg: EngineConfig,
    state: State,
    rules: Rules,
    now: Option<LogTime>,
    last_server_row: Option<LogTime>,
    offset: UtcOffset,
    detections: Vec<Detection>,
    notices: NoticeScheduler,
    summaries: Summaries,
    closed_days: Vec<Summary>,
    stats: EngineStats,
}

impl Engine {
    pub fn new(mut cfg: EngineConfig) -> Self {
        let r4 = &cfg.rules.r4;
        cfg.summary.finalize_delay_s = cfg
            .summary
            .finalize_delay_s
            .max(r4.notice_after_s)
            .max(r4.burst_s)
            .max(r4.server_s)
            .max(cfg.rules.r5.wait_s);
        Self {
            state: State::new(cfg.state.dup_ms),
            notices: NoticeScheduler::new(cfg.notice.clone()),
            cfg,
            rules: Rules::default(),
            now: None,
            last_server_row: None,
            offset: UtcOffset(0),
            detections: Vec::new(),
            summaries: Summaries::default(),
            closed_days: Vec::new(),
            stats: EngineStats::default(),
        }
    }

    pub fn now(&self) -> Option<LogTime> {
        self.now
    }

    /// Runs rule timers and closes summaries with deadlines strictly before `at`.
    /// Server records at `at` can still cancel rule timers scheduled for that instant.
    pub fn advance_to(&mut self, at: LogTime) {
        let delay_s = self.cfg.summary.finalize_delay_s;
        while let Some(cutoff) = self
            .summaries
            .next_close_deadline(delay_s)
            .filter(|&cutoff| cutoff < at)
        {
            self.advance_timers_to(cutoff);
            self.closed_days
                .extend(self.summaries.close_days(cutoff, delay_s));
        }
        self.advance_timers_to(at);
    }

    fn advance_timers_to(&mut self, at: LogTime) {
        if self.now.is_some_and(|now| now >= at) {
            return;
        }
        let first = self.detections.len();
        let power_offs = self
            .rules
            .advance(at, &self.state, &self.cfg.rules, &mut self.detections);
        self.stats.power_offs = self
            .stats
            .power_offs
            .saturating_add(power_offs.len() as u64);
        for cut in power_offs {
            self.summaries.power_off(
                cut.at,
                cut.offset,
                cut.tracker,
                cut.conn,
                cut.serial.as_ref(),
            );
        }
        self.record_detections(first);
        self.notices.advance_to(at);
        let long_losses = self
            .summaries
            .advance_to(at, self.offset, self.cfg.rules.r5.wait_s);
        self.stats.long_losses = self.stats.long_losses.saturating_add(long_losses);
        self.now = Some(at);
    }

    pub fn apply(&mut self, mut timed: Timed) {
        if matches!(timed.event, Event::PowerOffNotice { .. }) {
            self.record_driver_line(Some((timed.at, timed.offset)), None);
            let first = self.detections.len();
            self.rules
                .remember_driver_event(&timed, &self.cfg.rules, &mut self.detections);
            self.record_detections(first);
            return;
        }
        self.stats.server.record(None);
        let before = self.snapshot();
        timed.at = self.observe_server_line(timed.at, timed.offset, &before, None);
        timed.offset = self.offset;
        let update = self.state.apply(&timed);
        let after = self.snapshot();
        self.summaries.event(&timed, &before, &after, &update);
        if update.out_of_order
            && let Some(source) = self.summaries.source(None)
        {
            source.out_of_order = source.out_of_order.saturating_add(1);
        }
        let first = self.detections.len();
        self.rules.apply(
            &timed,
            &update.connections,
            &self.state,
            &self.cfg.rules,
            &mut self.detections,
        );
        self.record_detections(first);
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
        self.note_line_in_file(file, None, at, quality);
    }

    /// Uses the file's date for records without a readable header.
    pub fn note_line_in_file(
        &mut self,
        file: FileKind,
        file_date: Option<LogDate>,
        at: Option<(LogTime, UtcOffset)>,
        quality: LineQuality,
    ) {
        match file {
            FileKind::Server => {
                self.stats.server.record(Some(quality));
                if let Some((at, offset)) = at {
                    let before = self.snapshot();
                    self.observe_server_line(at, offset, &before, Some(quality));
                } else if let Some(source) = self.summary_source(file_date) {
                    count_server_record(source, Some(quality));
                }
            }
            FileKind::Driver => self.record_driver_record(at, file_date, Some(quality)),
        }
    }

    /// Driver records through this inclusive instant must arrive before server
    /// time can move beyond it.
    pub fn driver_needed_through(&self) -> Option<LogTime> {
        let now = self.now?;
        // Wall ticks may close summaries without changing the server's input day.
        let basis = self.last_server_row.unwrap_or(now);
        let local_ms =
            (basis.0.rem_euclid(DAY_MS) + i64::from(self.offset.0) * 60_000).rem_euclid(DAY_MS);
        let day_end = LogTime(basis.0.saturating_add(DAY_MS - 1 - local_ms));
        if self.cfg.rules.r4.exclude_power_off
            && let Some(deadline) = self.rules.r4_deadline()
        {
            return Some(deadline.min(day_end));
        }
        Some(day_end)
    }

    pub fn next_deadline(&self) -> Option<LogTime> {
        // An idle tick closes a summary as soon as the clock passes its cutoff.
        let summary_tick = self
            .summaries
            .next_close_deadline(self.cfg.summary.finalize_delay_s)
            .and_then(|cutoff| cutoff.0.checked_add(1).map(LogTime));
        self.rules
            .next_deadline()
            .into_iter()
            .chain(self.notices.next_deadline())
            .chain(summary_tick)
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
        self.last_server_row = None;
        self.summaries = Summaries::default();
        self.closed_days.clear();
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
        let run_pushes = self.stats.pushes;
        self.detections
            .drain(..)
            .map(Output::Detection)
            .chain(notices.into_iter().map(Output::Notice))
            .chain(self.closed_days.drain(..).map(|mut day| {
                // The app-run counter reflects deliveries when the summary is emitted.
                day.pushes = run_pushes;
                Output::DayClosed(Box::new(day))
            }))
            .collect()
    }

    pub fn live_notices(&self) -> Vec<Notice> {
        self.notices.live()
    }

    pub fn open_summaries(&self) -> Vec<Summary> {
        let mut days = self.summaries.open_summaries();
        for day in &mut days {
            day.pushes = self.stats.pushes;
        }
        days
    }

    /// Freezes pending prior days at end of input without advancing time.
    /// The latest observed day stays open if its normal cutoff has not arrived.
    pub fn finish_summaries(&mut self) {
        self.closed_days.extend(self.summaries.finish());
    }

    /// Additional parser or reader counters with no effect on observed time.
    pub fn summary_source(&mut self, date: Option<LogDate>) -> Option<&mut SummarySource> {
        self.summaries.source(date)
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot(self.now.unwrap_or(LogTime(0)))
    }

    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }

    fn observe_server_line(
        &mut self,
        at: LogTime,
        offset: UtcOffset,
        before: &Snapshot,
        quality: Option<LineQuality>,
    ) -> LogTime {
        let (effective, offset) = self
            .now
            .filter(|&now| at < now)
            .map_or((at, offset), |now| (now, self.offset));
        // A real row supplies the end of the observation interval. Split it
        // across midnight before advancing timers that can close the prior day.
        self.summaries
            .observe(effective, offset, before, self.cfg.summary.gap_s);
        if effective == at {
            self.advance_to(at);
            self.offset = offset;
        } else {
            self.stats.time_regressions = self.stats.time_regressions.saturating_add(1);
        }
        self.last_server_row = Some(effective);
        // Rows exactly at the cutoff close the day before their own facts apply.
        self.closed_days.extend(
            self.summaries
                .close_days(effective, self.cfg.summary.finalize_delay_s),
        );
        if let Some(source) = self.summaries.source(None) {
            count_server_record(source, quality);
            if effective != at {
                source.time_regressions = source.time_regressions.saturating_add(1);
            }
        }
        effective
    }

    fn record_detections(&mut self, first: usize) {
        for detection in &self.detections[first..] {
            self.summaries.detection(detection, self.offset);
            self.notices.advance_to(detection.at);
            let snapshot = self.state.snapshot(detection.at);
            self.notices.apply(detection, &snapshot);
        }
    }

    fn record_driver_line(
        &mut self,
        at: Option<(LogTime, UtcOffset)>,
        quality: Option<LineQuality>,
    ) {
        self.record_driver_record(at, None, quality);
    }

    fn record_driver_record(
        &mut self,
        at: Option<(LogTime, UtcOffset)>,
        file_date: Option<LogDate>,
        quality: Option<LineQuality>,
    ) {
        self.stats.driver.record(quality);
        let day = at.map(|(at, offset)| at.to_local(offset).0);
        if let Some(day) = day {
            self.stats
                .driver_days
                .entry(day)
                .or_default()
                .record(quality);
        }
        if let Some(source) = self.summary_source(day.or(file_date)) {
            source.driver.found = true;
            source.driver.lines = source.driver.lines.saturating_add(1);
            if quality == Some(LineQuality::NoHeader) {
                source.driver.no_header = source.driver.no_header.saturating_add(1);
            }
        }
    }
}

fn count_server_record(source: &mut SummarySource, quality: Option<LineQuality>) {
    source.lines = source.lines.saturating_add(1);
    let count = match quality {
        Some(LineQuality::Other) => &mut source.other,
        Some(LineQuality::Unparsed) => &mut source.unparsed,
        Some(LineQuality::NoHeader) => &mut source.no_header,
        None => return,
    };
    *count = count.saturating_add(1);
}
