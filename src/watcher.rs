//! Drives the watcher without owning filesystem I/O or the clock.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use openut_core::display::{
    self, CheckOutcome, DisplayModel, HubGate, HubLive, ServerFiles, WatchHealth,
};
use openut_core::engine::{Engine, EngineStats, FileKind as Source, LineQuality, Output};
use openut_core::event::Event;
use openut_core::state::{Phase, Snapshot};
use openut_core::summary::Summary;
use openut_core::time::{LogDate, LogTime};
use openut_hublog::header::FileKind;
use openut_hublog::map::{MapCounts, map_entry};
use openut_hublog::tail::{
    FileEvent, FollowEvent, LogFollower, LogSource, Record, RecordData, RecordPosition,
    parse_log_name,
};

use crate::config::WatchConfig;

mod runtime;
#[cfg(test)]
mod tests;

pub use runtime::watch;

const SUMMARY_INTERVAL_MS: i64 = 60_000;
const STALLED_AFTER_MS: i64 = 600_000;
const FORMAT_CHECK_LINES: u64 = 2_000;
const UNPARSED_LIMIT: u64 = 100;

pub enum SummaryWrite {
    Daily(Summary),
    BeforeRebuild(Summary),
}

pub struct WatchRound {
    pub outputs: Vec<Output>,
    pub summaries: Vec<SummaryWrite>,
    periodic_summary: bool,
    pub display: DisplayModel,
    pub health: WatchHealth,
    pub hub: HubLive,
    pub catching_up: bool,
    pub more: bool,
    pub diagnostics: Vec<String>,
}

type ProcessCheck<'a> = dyn FnMut(&[String]) -> Option<bool> + 'a;
type SummarySink<'a> = dyn FnMut(SummaryWrite) + 'a;

pub struct Watcher {
    config: WatchConfig,
    engine: Engine,
    follower: LogFollower,
    gate: HubGate,
    catching_up: bool,
    health: WatchHealth,
    read_failures: u64,
    next_process_check: Option<LogTime>,
    next_summary: Option<LogTime>,
    deferred_closed: Vec<SummaryWrite>,
    last_growth: Option<LogTime>,
    latest_server: Option<LogTime>,
    latest_driver: Option<LogTime>,
    suspended: bool,
    format: FormatObservation,
    numbered: BTreeSet<String>,
}

impl Watcher {
    pub fn new(config: WatchConfig) -> Self {
        let mut engine = Engine::new(config.engine.clone());
        engine.set_catching_up(true);
        Self {
            follower: LogFollower::new(config.lateness_ms),
            config,
            engine,
            gate: HubGate::default(),
            catching_up: true,
            health: WatchHealth::Ok,
            read_failures: 0,
            next_process_check: None,
            next_summary: None,
            deferred_closed: Vec::new(),
            last_growth: None,
            latest_server: None,
            latest_driver: None,
            suspended: false,
            format: FormatObservation::default(),
            numbered: BTreeSet::new(),
        }
    }

    pub fn now(&self) -> Option<LogTime> {
        self.engine.now()
    }

    pub fn stats(&self) -> &EngineStats {
        self.engine.stats()
    }

    pub fn open_summaries(&self) -> Vec<Summary> {
        self.engine.open_summaries()
    }

    pub fn snapshot(&self) -> Snapshot {
        self.engine.snapshot()
    }

    /// One bounded read and all currently deliverable records, using the supplied wall time.
    pub fn poll<S: LogSource>(
        &mut self,
        source: &S,
        now: LogTime,
        process: Option<&mut ProcessCheck<'_>>,
    ) -> WatchRound {
        self.poll_with_rebuild_sink(source, now, process, None)
    }

    fn poll_with_rebuild_sink<S: LogSource>(
        &mut self,
        source: &S,
        now: LogTime,
        mut process: Option<&mut ProcessCheck<'_>>,
        mut before_rebuild: Option<&mut SummarySink<'_>>,
    ) -> WatchRound {
        let mut summaries = Vec::new();
        let mut outputs = Vec::new();
        let mut diagnostics = Vec::new();
        if self.next_process_check.is_none_or(|next| now >= next)
            && let Some(check) = process.as_deref_mut()
        {
            self.probe(source, now, check, &mut diagnostics);
        }
        let read = self
            .follower
            .poll(source, now, self.engine.driver_needed_through());
        self.sync_gate();
        let more = match read {
            Ok(more) => {
                self.read_failures = 0;
                while let Some(event) = self
                    .follower
                    .next_event(now, self.engine.driver_needed_through())
                {
                    match event {
                        FollowEvent::Rebuild { file, name, .. } => {
                            self.rebuild(
                                if file == FileKind::Server {
                                    name.as_deref()
                                } else {
                                    None
                                },
                                true,
                                &mut summaries,
                                &mut outputs,
                                &mut before_rebuild,
                            );
                            diagnostics.push("ログの再構築を始めました。".to_owned());
                        }
                        FollowEvent::File { file, event } => {
                            self.file_event(
                                file,
                                event,
                                &mut summaries,
                                &mut outputs,
                                &mut diagnostics,
                                &mut before_rebuild,
                            );
                        }
                        FollowEvent::ReadComplete => {}
                        FollowEvent::Record {
                            record,
                            position,
                            late,
                        } => {
                            if record.file == FileKind::Server
                                && let Some(position) = position.as_ref()
                                && self.gate.before_record(&position.name, position.end)
                                && let Some(check) = process.as_deref_mut()
                            {
                                self.probe(source, now, check, &mut diagnostics);
                                self.sync_gate();
                            }
                            self.record(record, position.as_ref(), late, now, &mut diagnostics);
                        }
                    }
                }
                self.note_growth(now, &mut diagnostics);
                if let Some(at) = self
                    .follower
                    .advance_target(now, self.engine.driver_needed_through())
                {
                    self.engine.advance_to(at);
                }
                if self.catching_up
                    && self.follower.read_complete()
                    && !self.follower.has_pending_server_records()
                {
                    self.catching_up = false;
                    self.engine.set_catching_up(false);
                    self.last_growth = self.latest_server.map(|at| at.min(now));
                }
                if !self.catching_up
                    && !more
                    && !self.follower.server_grew()
                    && !self.follower.has_pending_server_records()
                {
                    self.engine.close_idle_summaries(now);
                }
                more
            }
            Err(_) => {
                self.read_failures = self.read_failures.saturating_add(1);
                diagnostics.push(format!(
                    "ログを読み取れませんでした。連続 {} 回。",
                    self.read_failures
                ));
                false
            }
        };
        self.drain_outputs(&mut outputs, &mut summaries);
        let periodic_summary =
            !self.catching_up && self.next_summary.is_none_or(|next| now >= next);
        if periodic_summary {
            self.next_summary = Some(after_ms(now, SUMMARY_INTERVAL_MS));
            summaries.extend(self.open_summaries().into_iter().map(SummaryWrite::Daily));
        }
        let snapshot = self.snapshot();
        let hub = self.gate.live(snapshot.hub);
        let health = self.update_health(&snapshot, hub, now, &mut diagnostics);
        let display = display::view(
            &snapshot,
            &self.engine.live_notices(),
            health,
            hub,
            self.catching_up,
        );
        WatchRound {
            outputs,
            summaries,
            periodic_summary,
            display,
            health,
            hub,
            catching_up: self.catching_up,
            more,
            diagnostics,
        }
    }

    fn probe<S: LogSource>(
        &mut self,
        source: &S,
        now: LogTime,
        check: &mut ProcessCheck<'_>,
        diagnostics: &mut Vec<String>,
    ) {
        let started = Instant::now();
        let before = server_files(source);
        let present = check(&self.config.hub_process_names);
        let after = server_files(source);
        diagnostics.push(format!(
            "VIVE Hub のプロセスとサーバーログ一覧を {:.1} ms で確認しました。",
            started.elapsed().as_secs_f64() * 1_000.0,
        ));
        let outcome = present.map(|present| {
            self.gate.process_checked(
                present,
                before.as_ref(),
                after.as_ref(),
                self.snapshot().hub,
                now,
            )
        });
        if outcome == Some(CheckOutcome::NameMismatch) {
            diagnostics.push("ログは追記されていますが、設定した名前のプロセスが見つかりません。ログから状態を判断します。".to_owned());
        }
        let delay = if outcome.is_none() || outcome == Some(CheckOutcome::RetrySoon) {
            self.config.poll_ms
        } else {
            self.config.process_check_s.saturating_mul(1_000)
        };
        self.next_process_check = Some(after_ms(now, i64::try_from(delay).unwrap_or(i64::MAX)));
    }

    fn sync_gate(&mut self) {
        let log = self.engine.snapshot().hub;
        self.engine
            .set_hub_down(display::hub_down(self.gate.live(log), log));
    }

    fn drain_outputs(&mut self, outputs: &mut Vec<Output>, summaries: &mut Vec<SummaryWrite>) {
        let drained = self.engine.drain();
        let writes = if self.catching_up {
            &mut self.deferred_closed
        } else {
            summaries.append(&mut self.deferred_closed);
            summaries
        };
        writes.extend(drained.iter().filter_map(|output| match output {
            Output::DayClosed(summary) => Some(SummaryWrite::Daily((**summary).clone())),
            _ => None,
        }));
        outputs.extend(drained);
    }

    fn rebuild(
        &mut self,
        name: Option<&str>,
        replaced: bool,
        summaries: &mut Vec<SummaryWrite>,
        outputs: &mut Vec<Output>,
        before_rebuild: &mut Option<&mut SummarySink<'_>>,
    ) {
        // Preserve outputs from the old segment. Rebuilding clears the engine queue.
        self.drain_outputs(outputs, summaries);
        if self.catching_up {
            // The interrupted catch-up did not finish rebuilding this generation's days.
            self.deferred_closed.clear();
        } else {
            let write = if replaced {
                SummaryWrite::BeforeRebuild
            } else {
                SummaryWrite::Daily
            };
            for action in self.open_summaries().into_iter().map(write) {
                if let Some(sink) = before_rebuild.as_deref_mut() {
                    sink(action);
                } else {
                    summaries.push(action);
                }
            }
        }
        self.engine.rebuild();
        self.engine.set_catching_up(true);
        self.gate.rebuild(name);
        self.sync_gate();
        self.catching_up = true;
        self.next_summary = None;
        self.latest_server = None;
        self.latest_driver = None;
        self.last_growth = None;
        self.format = FormatObservation {
            reported: self.format.reported,
            ..FormatObservation::default()
        };
        if let Some(source) = self.engine.summary_source(None) {
            source.rebuilds = source.rebuilds.saturating_add(1);
        }
    }

    fn file_event(
        &mut self,
        file: FileKind,
        event: FileEvent,
        summaries: &mut Vec<SummaryWrite>,
        outputs: &mut Vec<Output>,
        diagnostics: &mut Vec<String>,
        before_rebuild: &mut Option<&mut SummarySink<'_>>,
    ) {
        match event {
            FileEvent::Rotated {
                skipped_days: true, ..
            } if file == FileKind::Server => {
                self.rebuild(None, false, summaries, outputs, before_rebuild);
                diagnostics
                    .push("サーバーログの日付が飛んだため、状態を組み立て直します。".to_owned());
            }
            FileEvent::LateWrite { name, .. } => {
                let date = parse_log_name(&name).map(|name| name.date);
                if let Some(source) = self.engine.summary_source(date) {
                    let counter = if file == FileKind::Server {
                        &mut source.late_writes
                    } else {
                        &mut source.driver.late_writes
                    };
                    *counter = counter.saturating_add(1);
                }
                diagnostics.push("切り替え後のログに追記を検出しました。".to_owned());
            }
            FileEvent::Numbered { name } if self.numbered.insert(name.clone()) => {
                diagnostics.push(format!("連番付きのログを検出しました（{name}）。"));
            }
            _ => {}
        }
    }

    fn record(
        &mut self,
        record: Record,
        position: Option<&RecordPosition>,
        late: bool,
        now: LogTime,
        diagnostics: &mut Vec<String>,
    ) {
        let file = record.file;
        let engine_source = if file == FileKind::Server {
            Source::Server
        } else {
            Source::Driver
        };
        let file_date = position
            .and_then(|position| parse_log_name(&position.name))
            .map(|name| name.date);
        let entry = match record.data {
            RecordData::Entry(entry) => entry,
            RecordData::Meta(_) => {
                if let Some(source) = self.engine.summary_source(file_date) {
                    let counter = if file == FileKind::Server {
                        &mut source.meta
                    } else {
                        &mut source.driver.meta
                    };
                    *counter = counter.saturating_add(1);
                }
                self.record_decode(file, file_date, position);
                return;
            }
            RecordData::NoHeader(_) => {
                self.engine.note_line_in_file(
                    engine_source,
                    file_date,
                    None,
                    LineQuality::NoHeader,
                );
                self.record_decode(file, file_date, position);
                return;
            }
        };
        let date = entry.at.to_local(entry.offset).0;
        let mut counts = MapCounts::default();
        let timed = map_entry(&entry.as_entry(), file, &mut counts);
        let boundary_event = timed.as_ref().and_then(|timed| match timed.event {
            Event::ServerStarted => Some(Event::ServerStarted),
            Event::ServerStopped => Some(Event::ServerStopped),
            _ => None,
        });
        if file == FileKind::Server {
            self.format.observe(
                date,
                timed.is_some(),
                counts.server.unparsed > 0,
                diagnostics,
            );
        }
        if let Some(timed) = timed {
            self.engine.apply(timed);
        } else {
            let quality = if counts.server.unparsed > 0 || counts.driver.unparsed > 0 {
                LineQuality::Unparsed
            } else {
                LineQuality::Other
            };
            self.engine.note_line_in_file(
                engine_source,
                file_date,
                Some((entry.at, entry.offset)),
                quality,
            );
        }
        if let Some(source) = self
            .engine
            .summary_source((file == FileKind::Driver).then_some(date))
        {
            if file == FileKind::Driver {
                source.driver.late = source.driver.late.saturating_add(u64::from(late));
                source.driver.bad_offset = source
                    .driver
                    .bad_offset
                    .saturating_add(u64::from(entry.bad_offset));
            } else {
                source.bad_offset = source
                    .bad_offset
                    .saturating_add(u64::from(entry.bad_offset));
            }
        }
        self.record_decode(file, Some(date), position);
        if file == FileKind::Server {
            let started = self.gate.starting_since();
            if let Some(position) = position {
                self.gate.record_applied(
                    &position.name,
                    position.end,
                    self.engine.now().unwrap_or(entry.at),
                    boundary_event.as_ref(),
                    self.snapshot().hub,
                    now,
                );
            }
            if matches!(boundary_event, Some(Event::ServerStarted))
                && let Some(since) = started
                && self.gate.starting_since().is_none()
            {
                diagnostics.push(format!(
                    "VIVE Hub の検出から開始ログまで {} ms。",
                    now.0.saturating_sub(since.0)
                ));
            }
            if boundary_event.is_some() {
                self.sync_gate();
            }
        }
    }

    fn record_decode(
        &mut self,
        file: FileKind,
        date: Option<LogDate>,
        position: Option<&RecordPosition>,
    ) {
        if let Some(position) = position
            && let Some(source) = self.engine.summary_source(date)
        {
            if file == FileKind::Driver {
                source.driver.found = true;
                source.driver.long_lines = source
                    .driver
                    .long_lines
                    .saturating_add(position.decode.long_lines);
                source.driver.replaced_chars = source
                    .driver
                    .replaced_chars
                    .saturating_add(position.decode.replaced_chars);
            } else {
                source.long_lines = source.long_lines.saturating_add(position.decode.long_lines);
                source.replaced_chars = source
                    .replaced_chars
                    .saturating_add(position.decode.replaced_chars);
            }
        }
    }

    fn note_growth(&mut self, now: LogTime, diagnostics: &mut Vec<String>) {
        if self.follower.server_grew() && !self.catching_up {
            self.last_growth = Some(now);
        }
        for (file, last, label) in [
            (FileKind::Server, &mut self.latest_server, "サーバー"),
            (FileKind::Driver, &mut self.latest_driver, "ドライバー"),
        ] {
            let current = self.follower.last_log_time(file);
            if let Some(at) = current
                && (current != *last || file == FileKind::Server && self.follower.server_grew())
            {
                diagnostics.push(format!(
                    "{label}ログの読み取り時刻の差 {} ms。",
                    now.0.saturating_sub(at.0)
                ));
            }
            *last = current;
        }
    }

    fn update_health(
        &mut self,
        snapshot: &Snapshot,
        hub: HubLive,
        now: LogTime,
        diagnostics: &mut Vec<String>,
    ) -> WatchHealth {
        let down = display::hub_down(hub, snapshot.hub);
        if self.suspended && !down {
            self.last_growth = Some(now);
        }
        self.suspended = down;
        let health = if self.read_failures >= 3 {
            WatchHealth::Unreadable
        } else if !self.follower.server_found() && self.read_failures == 0 {
            WatchHealth::NotFound
        } else if self.gate.start_wait_exceeded(
            snapshot.hub,
            now,
            Duration::from_secs(self.config.start_wait_s),
        ) || self.format.mismatched()
        {
            WatchHealth::FormatMismatch
        } else if !down
            && !self.catching_up
            && snapshot
                .trackers
                .iter()
                .any(|tracker| tracker.phase != Phase::Disconnected)
            && self
                .last_growth
                .is_some_and(|at| now.0.saturating_sub(at.0) >= STALLED_AFTER_MS)
        {
            WatchHealth::Stalled
        } else {
            WatchHealth::Ok
        };
        if health != self.health {
            let reason = match health {
                WatchHealth::Ok => "ログ監視を確認しました。",
                WatchHealth::NotFound => "対象のサーバーログが見つかりません。",
                WatchHealth::Unreadable => "連続してログを読み取れませんでした。",
                WatchHealth::FormatMismatch => "ログ形式または起動時の記録を確認できません。",
                WatchHealth::Stalled => {
                    "接続中のトラッカーがありますが、サーバーログが 10 分間伸びていません。"
                }
            };
            diagnostics.push(reason.to_owned());
            self.health = health;
        }
        health
    }
}

#[derive(Default)]
struct FormatObservation {
    date: Option<LogDate>,
    lines: u64,
    known: u64,
    unparsed: u64,
    reported: u64,
}

impl FormatObservation {
    fn observe(
        &mut self,
        date: LogDate,
        known: bool,
        unparsed: bool,
        diagnostics: &mut Vec<String>,
    ) {
        if self.date.is_none_or(|old| date > old) {
            *self = Self {
                date: Some(date),
                reported: self.reported,
                ..Self::default()
            };
        }
        if self.date == Some(date) {
            self.lines = self.lines.saturating_add(1);
            self.known = self.known.saturating_add(u64::from(known));
            self.unparsed = self.unparsed.saturating_add(u64::from(unparsed));
        }
        if unparsed && self.reported < 20 {
            self.reported += 1;
            diagnostics.push(format!(
                "種類を判定できたログの値を読み取れませんでした（{} 件目）。",
                self.reported
            ));
        }
    }

    fn mismatched(&self) -> bool {
        self.unparsed > UNPARSED_LIMIT || self.lines > FORMAT_CHECK_LINES && self.known == 0
    }
}

fn after_ms(at: LogTime, millis: i64) -> LogTime {
    LogTime(at.0.saturating_add(millis))
}

fn server_files<S: LogSource>(source: &S) -> Option<ServerFiles> {
    Some(
        source
            .list()
            .ok()?
            .into_iter()
            .filter(|file| {
                parse_log_name(&file.name)
                    .is_some_and(|name| name.kind == FileKind::Server && !name.numbered)
            })
            .map(|file| (file.name, file.len))
            .collect(),
    )
}
