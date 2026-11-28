//! Summarize a recording in stream order, without retaining its raw reports.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use openut_core::time::{LogTime, UtcOffset};

use crate::decode::{DongleEvent, TrackerMessage};
use crate::record::{ReadError, TrackerLabel};
use crate::replay::{Replay, ReplayStats, ReplayStep};
use crate::tracker::{Link, TrackerTable, Transition};
use crate::view::{elapsed_text, map_text, tracking_text};

/// Parse a signed UTC offset such as +9, -05 or +09:30.
pub fn parse_utc_offset(text: &str) -> Option<UtcOffset> {
    let (sign, rest) = if let Some(rest) = text.strip_prefix('+') {
        (1, rest)
    } else {
        (-1, text.strip_prefix('-')?)
    };
    let (hours, minutes) = match rest.split_once(':') {
        Some((hours, minutes)) if minutes.len() == 2 => (hours, offset_number(minutes)?),
        Some(_) => return None,
        None => (rest, 0),
    };
    let hours = offset_number(hours)?;
    (hours <= 23 && minutes <= 59).then_some(UtcOffset(sign * (hours * 60 + minutes)))
}

fn offset_number(part: &str) -> Option<i16> {
    if (1..=2).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_digit()) {
        part.parse().ok()
    } else {
        None
    }
}

/// Replay and summarize a recording with elapsed and wall-clock times.
pub fn write_summary(
    input: impl BufRead,
    output: impl Write,
    offset: Option<UtcOffset>,
) -> Result<(), ReadError> {
    let mut replay = Replay::new(input)?;
    let mut summary = Summary {
        last: Stamp {
            mono_us: 0,
            wall: Some(LogTime(replay.header().started_wall_ms)),
        },
        trackers: BTreeMap::new(),
    };
    while let Some(step) = replay.read_step().map_err(ReadError::Io)? {
        summary.observe(&step);
    }

    summary
        .write(output, offset, replay.table(), replay.stats())
        .map_err(ReadError::Io)
}

#[derive(Clone, Copy)]
struct Stamp {
    mono_us: u64,
    wall: Option<LogTime>,
}

impl Stamp {
    fn at(self, mono_us: u64) -> Self {
        let delta_us = i128::from(mono_us) - i128::from(self.mono_us);
        let wall = self.wall.and_then(|wall| {
            i64::try_from(i128::from(wall.0) + delta_us.div_euclid(1_000))
                .ok()
                .map(LogTime)
        });
        Self { mono_us, wall }
    }

    fn text(self, offset: Option<UtcOffset>) -> String {
        let elapsed = elapsed_text(self.mono_us);
        match (self.wall, offset) {
            (None, _) => format!("{elapsed} 秒"),
            (Some(wall), None) => format!("{elapsed} 秒 ({wall} UTC)"),
            (Some(wall), Some(offset)) => {
                let (date, clock) = wall.to_local(offset);
                let sign = if offset.0 < 0 { '-' } else { '+' };
                let minutes = offset.0.unsigned_abs();
                format!(
                    "{elapsed} 秒 ({wall} UTC / {date} {clock} {sign}{:02}:{:02})",
                    minutes / 60,
                    minutes % 60,
                )
            }
        }
    }
}

struct Summary {
    last: Stamp,
    trackers: BTreeMap<char, TrackerSummary>,
}

impl Summary {
    fn observe(&mut self, step: &ReplayStep) {
        let now = Stamp {
            wall: step
                .wall_ms
                .map(LogTime)
                .or_else(|| self.last.at(step.mono_us).wall),
            mono_us: step.mono_us,
        };
        self.last = now;
        for transition in &step.transitions {
            let tracker = self.trackers.entry(transition.tracker.0).or_default();
            if transition.to == Link::Unpaired
                && matches!(&step.event, Some(DongleEvent::Text { tracker, .. }) if *tracker == transition.tracker)
                && let Some(interval) = &mut tracker.receiving
            {
                interval.texts += 1;
            }
            tracker.transition(transition, now);
        }

        match &step.event {
            Some(DongleEvent::Pair { tracker, .. }) => {
                let tracker = self.trackers.entry(tracker.0).or_default();
                tracker.saw_pair = true;
                tracker.seen_at(now);
            }
            Some(DongleEvent::Pose { tracker, sample }) => {
                let tracker = self.tracker_data(*tracker, now);
                tracker
                    .pose_status
                    .observe(i64::from(sample.tracking_status), now);
                if let Some(interval) = &mut tracker.receiving {
                    interval.poses += 1;
                }
            }
            Some(DongleEvent::Text { tracker, message }) => {
                let tracker = self.tracker_data(*tracker, now);
                if let TrackerMessage::SlamStatus { key: 6, value } = message {
                    tracker.slam_status.observe(*value, now);
                }
                if let Some(interval) = &mut tracker.receiving {
                    interval.texts += 1;
                }
            }
            Some(DongleEvent::OtherTrackerData { tracker, .. }) => {
                self.tracker_data(*tracker, now);
            }
            _ => {
                if let Some(tracker) = step.omitted_tracker {
                    self.tracker_data(tracker, now);
                }
            }
        }
    }

    fn tracker_data(&mut self, label: TrackerLabel, now: Stamp) -> &mut TrackerSummary {
        let tracker = self.trackers.entry(label.0).or_default();
        tracker.saw_tracker_data = true;
        tracker.seen_at(now);
        tracker
    }

    fn write(
        &self,
        mut out: impl Write,
        offset: Option<UtcOffset>,
        table: &TrackerTable,
        stats: ReplayStats,
    ) -> io::Result<()> {
        let pair_only = self
            .trackers
            .values()
            .filter(|tracker| tracker.saw_pair && !tracker.saw_tracker_data)
            .count();
        let lost = stats.estimated_lost.map_or_else(
            || "不明（締めの行に記録なし）".to_owned(),
            |n| format!("{n} 件"),
        );
        writeln!(out, "記録の終わり: {}", self.last.text(offset))?;
        writeln!(
            out,
            "受信報告: {} 件、不明 {} 件、不正 {} 件、省略 {} 件",
            stats.reports, stats.unknown_reports, stats.malformed_reports, stats.omitted_reports,
        )?;
        writeln!(
            out,
            "記録の行: 不明 {} 件、不正 {} 件",
            stats.rows.unknown, stats.rows.malformed,
        )?;
        writeln!(out, "取りこぼし（推定）: {lost}")?;
        writeln!(
            out,
            "時刻が戻った行 (time_reversed): {}",
            stats.time_reversed
        )?;
        writeln!(out, "ペアリングの知らせだけに現れた名前: {pair_only}")?;

        for (&label, tracker) in &self.trackers {
            writeln!(out, "トラッカー {label}")?;
            tracker.write(&mut out, self.last, offset)?;
            if let Some(view) = table.view(TrackerLabel(label)) {
                writeln!(out, "  {}", map_text(view))?;
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct TrackerSummary {
    first: Option<Stamp>,
    last: Option<Stamp>,
    saw_pair: bool,
    saw_tracker_data: bool,
    receiving: Option<Interval>,
    completed: Vec<(Interval, Stamp)>,
    pose_status: TrackingStats,
    slam_status: TrackingStats,
}

impl TrackerSummary {
    fn seen_at(&mut self, now: Stamp) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    fn transition(&mut self, transition: &Transition, now: Stamp) {
        if transition.to == Link::Receiving {
            self.receiving = Some(Interval {
                from: now.at(transition.at_us),
                poses: 0,
                texts: 0,
            });
        } else if let Some(interval) = self.receiving.take() {
            let anchor = if transition.to == Link::Silent {
                self.last.unwrap_or(now)
            } else {
                now
            };
            self.completed.push((interval, anchor.at(transition.at_us)));
        }
    }

    fn write(
        &self,
        out: &mut impl Write,
        recording_end: Stamp,
        offset: Option<UtcOffset>,
    ) -> io::Result<()> {
        if let (Some(first), Some(last)) = (self.first, self.last) {
            writeln!(
                out,
                "  見えた時間: {} 〜 {}",
                first.text(offset),
                last.text(offset),
            )?;
        }
        for (interval, end) in &self.completed {
            interval.write(out, *end, offset, "")?;
        }
        if let Some(interval) = &self.receiving {
            interval.write(out, recording_end, offset, "（記録終了時も受信中）")?;
        }
        self.pose_status.write(out, "姿勢", offset)?;
        self.slam_status.write(out, "P61:6", offset)
    }
}

struct Interval {
    from: Stamp,
    poses: u64,
    texts: u64,
}

impl Interval {
    fn write(
        &self,
        out: &mut impl Write,
        end: Stamp,
        offset: Option<UtcOffset>,
        suffix: &str,
    ) -> io::Result<()> {
        writeln!(
            out,
            "  受信中: {} 〜 {}{suffix}。姿勢 {} 件、文字列 {} 件",
            self.from.text(offset),
            end.text(offset),
            self.poses,
            self.texts,
        )
    }
}

#[derive(Default)]
struct TrackingStats {
    last: Option<i64>,
    counts: BTreeMap<i64, u64>,
    changes: Vec<(Stamp, i64)>,
}

impl TrackingStats {
    fn observe(&mut self, status: i64, now: Stamp) {
        *self.counts.entry(status).or_default() += 1;
        if self.last != Some(status) {
            self.changes.push((now, status));
            self.last = Some(status);
        }
    }

    fn write(
        &self,
        out: &mut impl Write,
        source: &str,
        offset: Option<UtcOffset>,
    ) -> io::Result<()> {
        if self.counts.is_empty() {
            return Ok(());
        }
        let counts: Vec<_> = self
            .counts
            .iter()
            .map(|(&status, count)| format!("{} {count} 件", tracking_text(status)))
            .collect();
        writeln!(out, "  追跡の状態（{source}）: {}", counts.join("、"))?;
        for (at, status) in &self.changes {
            writeln!(out, "    {} {}", at.text(offset), tracking_text(*status))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_signed_hour_and_minute_offsets() {
        for (text, minutes) in [("+9", 540), ("+09:30", 570), ("-03:30", -210), ("+0", 0)] {
            assert_eq!(parse_utc_offset(text), Some(UtcOffset(minutes)));
        }
        for text in ["9", "+24", "+1.5", "+09:3", "+09:60", "+010", "+09:+9"] {
            assert_eq!(parse_utc_offset(text), None);
        }
    }

    #[test]
    fn displays_elapsed_time_and_both_clock_times_across_midnight() {
        let stamp = Stamp {
            mono_us: 0,
            wall: Some(LogTime(86_399_000)),
        }
        .at(1_500_000);
        assert_eq!(
            stamp.text(Some(UtcOffset(-30))),
            "1.500 秒 (1970-01-02 00:00:00.500 UTC / 1970-01-01 23:30:00.500 -00:30)",
        );
    }
}
