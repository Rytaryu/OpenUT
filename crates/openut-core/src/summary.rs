//! Daily observation summaries, based on server rows and independent of file names.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;

use crate::event::{Event, Timed};
use crate::rules::{Detection, RuleId, Step, after};
use crate::state::{
    ConnId, ConnectionChangeKind, Phase, Snapshot, StateTally, StateUpdate, TrackerView,
};
use crate::time::{LogDate, LogTime, UtcOffset};
use crate::tracker::{Serial, TrackerKey};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SummaryConfig {
    pub gap_s: u64,
    pub finalize_delay_s: u64,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            gap_s: 600,
            finalize_delay_s: 600,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Summary {
    pub schema: u8,
    pub date: String,
    #[cfg_attr(feature = "serde", serde(rename = "final"))]
    pub is_final: bool,
    pub observed_until: String,
    pub unobserved_s: f64,
    pub source: SummarySource,
    pub trackers: Vec<TrackerSummary>,
    pub host_starts: u64,
    pub faults: SummaryFaults,
    pub detections: DetectionCounts,
    pub pushes: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SummarySource {
    pub lines: u64,
    pub meta: u64,
    pub other: u64,
    pub unparsed: u64,
    pub no_header: u64,
    pub bad_offset: u64,
    pub replaced_chars: u64,
    pub long_lines: u64,
    pub time_regressions: u64,
    pub late_writes: u64,
    pub out_of_order: u64,
    pub rebuilds: u64,
    pub driver: DriverSource,
}

impl SummarySource {
    fn merge(&mut self, other: Self) {
        self.lines = self.lines.saturating_add(other.lines);
        self.meta = self.meta.saturating_add(other.meta);
        self.other = self.other.saturating_add(other.other);
        self.unparsed = self.unparsed.saturating_add(other.unparsed);
        self.no_header = self.no_header.saturating_add(other.no_header);
        self.bad_offset = self.bad_offset.saturating_add(other.bad_offset);
        self.replaced_chars = self.replaced_chars.saturating_add(other.replaced_chars);
        self.long_lines = self.long_lines.saturating_add(other.long_lines);
        self.time_regressions = self.time_regressions.saturating_add(other.time_regressions);
        self.late_writes = self.late_writes.saturating_add(other.late_writes);
        self.out_of_order = self.out_of_order.saturating_add(other.out_of_order);
        self.rebuilds = self.rebuilds.saturating_add(other.rebuilds);
        self.driver.merge(other.driver);
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DriverSource {
    pub found: bool,
    pub lines: u64,
    pub meta: u64,
    pub no_header: u64,
    pub bad_offset: u64,
    pub replaced_chars: u64,
    pub long_lines: u64,
    pub late: u64,
    pub late_writes: u64,
}

impl DriverSource {
    fn merge(&mut self, other: Self) {
        self.found |= other.found;
        self.lines = self.lines.saturating_add(other.lines);
        self.meta = self.meta.saturating_add(other.meta);
        self.no_header = self.no_header.saturating_add(other.no_header);
        self.bad_offset = self.bad_offset.saturating_add(other.bad_offset);
        self.replaced_chars = self.replaced_chars.saturating_add(other.replaced_chars);
        self.long_lines = self.long_lines.saturating_add(other.long_lines);
        self.late = self.late.saturating_add(other.late);
        self.late_writes = self.late_writes.saturating_add(other.late_writes);
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SummaryFaults {
    pub wifi_info_missing: u64,
    pub unexpected_disconnects: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(rename_all = "UPPERCASE")
)]
pub struct DetectionCounts {
    pub r1: u64,
    pub r2: u64,
    pub r3: u64,
    pub r4: u64,
    pub r5: u64,
}

impl DetectionCounts {
    fn record(&mut self, rule: RuleId) {
        let count = match rule {
            RuleId::R1 => &mut self.r1,
            RuleId::R2 => &mut self.r2,
            RuleId::R3 => &mut self.r3,
            RuleId::R4 => &mut self.r4,
            RuleId::R5 => &mut self.r5,
        };
        *count = count.saturating_add(1);
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct TrackerSummary {
    pub serial: Option<String>,
    pub slots: Vec<u16>,
    pub hub_number: Option<u16>,
    pub connected_s: f64,
    pub in_use_s: f64,
    pub tracking_s: f64,
    pub tracking_ratio: Option<f64>,
    pub sessions: u64,
    pub first_pose_s: Vec<f64>,
    pub losses: u64,
    pub long_losses: u64,
    pub unexpected_disconnects: u64,
    pub power_offs: u64,
    pub overspeed: u64,
    pub host_count: u64,
}

#[derive(Clone, Copy)]
struct Observation {
    at: LogTime,
    offset: UtcOffset,
}

impl Observation {
    fn date(self) -> LogDate {
        self.at.to_local(self.offset).0
    }
}

#[derive(Default)]
pub(crate) struct Summaries {
    last: Option<Observation>,
    closed_through: Option<LogDate>,
    days: BTreeMap<LogDate, Day>,
    unplaced_source: SummarySource,
    active: BTreeMap<ConnId, Active>,
}

impl Summaries {
    /// Charge the interval to the state after the previous real server row.
    pub(crate) fn observe(
        &mut self,
        at: LogTime,
        offset: UtcOffset,
        before: &Snapshot,
        gap_s: u64,
    ) {
        let now = Observation { at, offset };
        let date = now.date();
        if self.is_closed(date) {
            return;
        }
        if let Some(previous) = self.last {
            let previous_date = previous.date();
            if at < previous.at || date < previous_date {
                return;
            }
            let gap = elapsed_ms(previous.at, at) > gap_s.saturating_mul(1_000);
            if date == previous_date {
                self.days
                    .entry(date)
                    .or_default()
                    .span(previous.at, at, before, gap);
            } else {
                let boundary = next_midnight(previous_date, offset);
                let old_end = boundary.clamp(previous.at, at);
                let day_start = midnight(date, offset).max(old_end);
                // The prior day may already have closed on a wall-clock tick.
                if let Some(day) = self.days.get_mut(&previous_date) {
                    day.span(previous.at, old_end, before, gap);
                    day.crossed_midnight = old_end == boundary;
                }
                self.days
                    .entry(date)
                    .or_default()
                    .span(day_start, at, before, gap);
            }
        } else {
            let day = self.days.entry(date).or_default();
            day.unobserved_ms = elapsed_ms(midnight(date, offset), at);
            day.source.merge(mem::take(&mut self.unplaced_source));
        }

        self.days.entry(date).or_default().observed = Some(now);
        self.last = Some(now);
    }

    pub(crate) fn event(
        &mut self,
        timed: &Timed,
        before: &Snapshot,
        after: &Snapshot,
        update: &StateUpdate,
    ) {
        let date = timed.at.to_local(timed.offset).0;
        if self.is_closed(date) {
            return;
        }

        // Serial evidence can apply to earlier rows for the same connection.
        // It can also arrive after midnight while the prior day is still open.
        for tracker in after
            .trackers
            .iter()
            .filter(|tracker| tracker.phase != Phase::Disconnected)
        {
            if let Some(serial) = &tracker.serial {
                for day in self.days.values_mut() {
                    if let Some(row) = day.connections.get_mut(&RowKey::Conn(tracker.conn)) {
                        row.serial = Some(serial.clone());
                    }
                }
            }
        }

        let day = self.days.entry(date).or_default();
        for change in &update.connections {
            let tracker = &change.tracker;
            match change.kind {
                ConnectionChangeKind::Started => {
                    self.active.insert(tracker.conn, Active::new(change.at));
                    if matches!(timed.event, Event::TrackerAdded { .. }) {
                        let row = day.tracker(tracker);
                        row.totals.sessions = row.totals.sessions.saturating_add(1);
                    }
                }
                ConnectionChangeKind::Ended(_) => {
                    self.active.remove(&tracker.conn);
                    // Its hub number remains the number of the last connection that day.
                    if let Some(row) = day.connections.get_mut(&RowKey::Conn(tracker.conn)) {
                        row.observe(tracker);
                    }
                }
            }
        }

        if matches!(timed.event, Event::HostAssigned { .. }) {
            day.host_starts = day.host_starts.saturating_add(1);
        }

        for tracker in after
            .trackers
            .iter()
            .filter(|tracker| tracker.phase != Phase::Disconnected)
        {
            let active = self
                .active
                .entry(tracker.conn)
                .or_insert_with(|| Active::new(timed.at));
            let row = day.tracker(tracker);
            if matches!(timed.event, Event::HostAssigned { tracker: key } if tracker.key == key) {
                active.first_pose_from = timed.at;
                row.totals.host_count = row.totals.host_count.saturating_add(1);
            }

            let previous = before
                .trackers
                .iter()
                .find(|previous| previous.conn == tracker.conn);
            if tracker.phase == Phase::Tracking
                && !previous.is_some_and(|old| old.tracked_since_connect)
            {
                row.totals
                    .first_pose_ms
                    .push((timed.at, elapsed_ms(active.first_pose_from, timed.at)));
            }
            if tracker.phase == Phase::Relocalizing
                && previous.is_some_and(|old| old.phase == Phase::Tracking)
            {
                row.totals.losses = row.totals.losses.saturating_add(1);
                active.loss_at = Some(Observation {
                    at: timed.at,
                    offset: timed.offset,
                });
            } else if tracker.phase != Phase::Relocalizing {
                active.loss_at = None;
            }
            if update.tally == Some(StateTally::Overspeed)
                && matches!(timed.event, Event::Oet { tracker: key, .. } if tracker.key == key)
            {
                row.totals.overspeed = row.totals.overspeed.saturating_add(1);
            }
        }
    }

    pub(crate) fn advance_to(&mut self, at: LogTime, _offset: UtcOffset, wait_s: u64) -> u64 {
        let mut matured = 0_u64;
        for (&conn, active) in &mut self.active {
            if let Some(loss) = active.loss_at
                && elapsed_ms(loss.at, at) > wait_s.saturating_mul(1_000)
            {
                matured = matured.saturating_add(1);
                if let Some(day) = self.days.get_mut(&loss.date())
                    && let Some(row) = day.connections.get_mut(&RowKey::Conn(conn))
                {
                    row.totals.long_losses = row.totals.long_losses.saturating_add(1);
                }
                active.loss_at = None;
            }
        }
        matured
    }

    pub(crate) fn detection(&mut self, detection: &Detection, offset: UtcOffset) {
        if detection.step != Step::Raised {
            return;
        }
        let date = detection.at.to_local(offset).0;
        if self.is_closed(date) {
            return;
        }
        let day = self.days.entry(date).or_default();
        day.detections.record(detection.rule);
        if detection.rule == RuleId::R4
            && let Some(tracker) = detection.tracker
        {
            let row = day.target(tracker, detection.conn, detection.serial.as_ref());
            row.totals.unexpected_disconnects = row.totals.unexpected_disconnects.saturating_add(1);
        }
    }

    pub(crate) fn power_off(
        &mut self,
        at: LogTime,
        offset: UtcOffset,
        tracker: TrackerKey,
        conn: Option<ConnId>,
        serial: Option<&Serial>,
    ) {
        let date = at.to_local(offset).0;
        if self.is_closed(date) {
            return;
        }
        let row = self
            .days
            .entry(date)
            .or_default()
            .target(tracker, conn, serial);
        row.totals.power_offs = row.totals.power_offs.saturating_add(1);
    }

    pub(crate) fn source(&mut self, date: Option<LogDate>) -> Option<&mut SummarySource> {
        match date.or_else(|| self.last.map(Observation::date)) {
            Some(date) if self.is_closed(date) => None,
            Some(date) => Some(&mut self.days.entry(date).or_default().source),
            None => Some(&mut self.unplaced_source),
        }
    }

    pub(crate) fn is_closed(&self, date: LogDate) -> bool {
        self.closed_through.is_some_and(|last| date <= last)
    }

    pub(crate) fn next_close_deadline(&self, delay_s: u64) -> Option<LogTime> {
        self.days
            .iter()
            .find_map(|(&date, day)| day.close_deadline(date, delay_s))
    }

    pub(crate) fn close_days(&mut self, at: LogTime, delay_s: u64) -> Vec<Summary> {
        let due = self
            .days
            .iter()
            .filter_map(|(&date, day)| Some((date, day.close_deadline(date, delay_s)?)))
            .take_while(|&(_, deadline)| deadline <= at)
            .last();
        due.map_or_else(Vec::new, |(date, _)| self.close_through(date))
    }

    /// Finalizes prior days at EOF. The last observed day keeps its normal deadline.
    pub(crate) fn finish(&mut self) -> Vec<Summary> {
        let prior = self
            .last
            .and_then(|last| last.date().days_since_epoch())
            .and_then(|last| LogDate::from_days_since_epoch(last - 1));
        prior.map_or_else(Vec::new, |date| self.close_through(date))
    }

    fn close_through(&mut self, date: LogDate) -> Vec<Summary> {
        self.closed_through = Some(self.closed_through.map_or(date, |old| old.max(date)));
        self.days
            .extract_if(..=date, |_, _| true)
            .filter_map(|(date, day)| day.summary(date, true))
            .collect()
    }

    pub(crate) fn open_summaries(&self) -> Vec<Summary> {
        self.days
            .iter()
            .filter_map(|(&date, day)| day.summary(date, false))
            .collect()
    }
}

struct Active {
    first_pose_from: LogTime,
    loss_at: Option<Observation>,
}

impl Active {
    fn new(start: LogTime) -> Self {
        Self {
            first_pose_from: start,
            loss_at: None,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RowKey {
    Conn(ConnId),
    Slot(TrackerKey),
}

#[derive(Default)]
struct Day {
    observed: Option<Observation>,
    crossed_midnight: bool,
    source: SummarySource,
    unobserved_ms: u64,
    connections: BTreeMap<RowKey, ConnectionRow>,
    host_starts: u64,
    detections: DetectionCounts,
}

impl Day {
    fn close_deadline(&self, date: LogDate, delay_s: u64) -> Option<LogTime> {
        let observed = self.observed?;
        Some(after(next_midnight(date, observed.offset), delay_s))
    }

    fn span(&mut self, from: LogTime, to: LogTime, before: &Snapshot, unobserved: bool) {
        let duration = elapsed_ms(from, to);
        if duration == 0 {
            return;
        }
        if unobserved {
            self.unobserved_ms = self.unobserved_ms.saturating_add(duration);
            return;
        }
        for tracker in before
            .trackers
            .iter()
            .filter(|tracker| tracker.phase != Phase::Disconnected)
        {
            let row = self.tracker(tracker);
            row.totals.connected_ms = row.totals.connected_ms.saturating_add(duration);
            if tracker.tracked_since_connect {
                row.totals.in_use_ms = row.totals.in_use_ms.saturating_add(duration);
            }
            if tracker.phase == Phase::Tracking {
                row.totals.tracking_ms = row.totals.tracking_ms.saturating_add(duration);
            }
        }
    }

    fn tracker(&mut self, tracker: &TrackerView) -> &mut ConnectionRow {
        let row = self.target(tracker.key, Some(tracker.conn), tracker.serial.as_ref());
        row.observe(tracker);
        row
    }

    fn target(
        &mut self,
        tracker: TrackerKey,
        conn: Option<ConnId>,
        serial: Option<&Serial>,
    ) -> &mut ConnectionRow {
        let key = conn.map_or(RowKey::Slot(tracker), RowKey::Conn);
        let row = self
            .connections
            .entry(key)
            .or_insert_with(|| ConnectionRow {
                tracker,
                serial: None,
                hub_number: None,
                totals: Totals::default(),
            });
        if let Some(serial) = serial {
            row.serial = Some(serial.clone());
        }
        row
    }

    fn summary(&self, date: LogDate, is_final: bool) -> Option<Summary> {
        let observed = self.observed?;
        let observed_until = if self.crossed_midnight {
            format!("{} 00:00:00.000", next_date(date))
        } else {
            let (date, clock) = observed.at.to_local(observed.offset);
            format!("{date} {clock}")
        };
        Some(Summary {
            schema: 1,
            date: date.to_string(),
            is_final,
            observed_until,
            unobserved_s: seconds(self.unobserved_ms),
            source: self.source.clone(),
            trackers: self.trackers(),
            host_starts: self.host_starts,
            faults: SummaryFaults {
                wifi_info_missing: self.detections.r1,
                unexpected_disconnects: self.detections.r4,
            },
            detections: self.detections.clone(),
            pushes: 0,
        })
    }

    fn trackers(&self) -> Vec<TrackerSummary> {
        let mut rows = BTreeMap::<_, AccumulatedRow>::new();
        for row in self.connections.values() {
            let key = row
                .serial
                .as_ref()
                .map_or(TrackerRowKey::Slot(row.tracker), TrackerRowKey::Serial);
            let destination = rows.entry(key).or_default();
            destination.slots.insert(row.tracker.0);
            destination.hub_number = row.hub_number;
            destination.totals.merge(&row.totals);
        }
        rows.into_iter()
            .map(|(key, row)| row.summary(key))
            .collect()
    }
}

struct ConnectionRow {
    tracker: TrackerKey,
    serial: Option<Serial>,
    hub_number: Option<u16>,
    totals: Totals,
}

impl ConnectionRow {
    fn observe(&mut self, tracker: &TrackerView) {
        if let Some(serial) = &tracker.serial {
            self.serial = Some(serial.clone());
        }
        if let Some(number) = tracker.hub_number {
            self.hub_number = Some(number.0);
        }
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum TrackerRowKey<'a> {
    Serial(&'a Serial),
    Slot(TrackerKey),
}

#[derive(Default)]
struct AccumulatedRow {
    slots: BTreeSet<u16>,
    hub_number: Option<u16>,
    totals: Totals,
}

impl AccumulatedRow {
    fn summary(self, key: TrackerRowKey<'_>) -> TrackerSummary {
        let mut totals = self.totals;
        totals.first_pose_ms.sort_by_key(|(at, _)| *at);
        TrackerSummary {
            serial: match key {
                TrackerRowKey::Serial(serial) => Some(serial.0.clone()),
                TrackerRowKey::Slot(_) => None,
            },
            slots: self.slots.into_iter().collect(),
            hub_number: self.hub_number,
            connected_s: seconds(totals.connected_ms),
            in_use_s: seconds(totals.in_use_ms),
            tracking_s: seconds(totals.tracking_ms),
            tracking_ratio: (totals.in_use_ms != 0)
                .then(|| totals.tracking_ms as f64 / totals.in_use_ms as f64),
            sessions: totals.sessions,
            first_pose_s: totals
                .first_pose_ms
                .into_iter()
                .map(|(_, duration)| seconds(duration))
                .collect(),
            losses: totals.losses,
            long_losses: totals.long_losses,
            unexpected_disconnects: totals.unexpected_disconnects,
            power_offs: totals.power_offs,
            overspeed: totals.overspeed,
            host_count: totals.host_count,
        }
    }
}

#[derive(Default)]
struct Totals {
    connected_ms: u64,
    in_use_ms: u64,
    tracking_ms: u64,
    sessions: u64,
    first_pose_ms: Vec<(LogTime, u64)>,
    losses: u64,
    long_losses: u64,
    unexpected_disconnects: u64,
    power_offs: u64,
    overspeed: u64,
    host_count: u64,
}

impl Totals {
    fn merge(&mut self, other: &Self) {
        self.connected_ms = self.connected_ms.saturating_add(other.connected_ms);
        self.in_use_ms = self.in_use_ms.saturating_add(other.in_use_ms);
        self.tracking_ms = self.tracking_ms.saturating_add(other.tracking_ms);
        self.sessions = self.sessions.saturating_add(other.sessions);
        self.first_pose_ms.extend_from_slice(&other.first_pose_ms);
        self.losses = self.losses.saturating_add(other.losses);
        self.long_losses = self.long_losses.saturating_add(other.long_losses);
        self.unexpected_disconnects = self
            .unexpected_disconnects
            .saturating_add(other.unexpected_disconnects);
        self.power_offs = self.power_offs.saturating_add(other.power_offs);
        self.overspeed = self.overspeed.saturating_add(other.overspeed);
        self.host_count = self.host_count.saturating_add(other.host_count);
    }
}

fn elapsed_ms(from: LogTime, to: LogTime) -> u64 {
    u64::try_from(i128::from(to.0) - i128::from(from.0)).unwrap_or_default()
}

fn seconds(ms: u64) -> f64 {
    ms as f64 / 1_000.0
}

fn midnight(date: LogDate, offset: UtcOffset) -> LogTime {
    LogTime::from_local(date, 0, 0, 0, 0, offset).unwrap_or(LogTime(i64::MIN))
}

fn next_date(date: LogDate) -> LogDate {
    date.days_since_epoch()
        .and_then(|days| LogDate::from_days_since_epoch(days + 1))
        .unwrap_or(date)
}

fn next_midnight(date: LogDate, offset: UtcOffset) -> LogTime {
    LogTime::from_local(next_date(date), 0, 0, 0, 0, offset).unwrap_or(LogTime(i64::MAX))
}
