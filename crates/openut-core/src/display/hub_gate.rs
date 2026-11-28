//! Distinguishes the current Hub session from records left by a stopped process.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::event::Event;
use crate::state::HubState;
use crate::time::LogTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubLive {
    Unknown,
    Running,
    Stopped,
    Starting,
}

/// Whether the log-derived values should be withheld from the display and push sinks.
pub fn hub_down(live: HubLive, log: HubState) -> bool {
    matches!(live, HubLive::Stopped | HubLive::Starting)
        || (live == HubLive::Unknown && log == HubState::Stopped)
}

/// Server filenames and read-only file lengths, supplied by the caller.
pub type ServerFiles = BTreeMap<String, u64>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogBoundary {
    /// No process absence has been confirmed.
    Unmonitored,
    /// This record was present when the process was last confirmed absent.
    Existing,
    /// This record was written after that check.
    Appended,
    /// The file was replaced and its former byte positions no longer apply.
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckOutcome {
    Counted,
    /// A failed or changing file inventory makes this absence inconclusive.
    RetrySoon,
    /// A confirming check found no named process despite new log data.
    NameMismatch,
    /// The process name check has already been disabled for this run.
    Ignored,
}

#[derive(Default)]
pub struct HubGate {
    last_present: Option<bool>,
    waiting_for_start: bool,
    boundaries: Option<BTreeMap<String, Option<u64>>>,
    latest_applied: Option<LogTime>,
    stopped_mark: Option<LogTime>,
    starting_since: Option<LogTime>,
    untrusted: bool,
    awaiting_recheck: bool,
}

impl HubGate {
    pub fn live(&self, log: HubState) -> HubLive {
        match (self.untrusted, self.last_present) {
            (true, _) | (_, None) => HubLive::Unknown,
            (false, Some(false)) => HubLive::Stopped,
            (false, Some(true)) if self.waiting_for_start || log == HubState::Stopped => {
                HubLive::Starting
            }
            (false, Some(true)) => HubLive::Running,
        }
    }

    pub fn boundary(&self, file: &str, end: u64) -> LogBoundary {
        match self.boundaries.as_ref() {
            None => LogBoundary::Unmonitored,
            Some(boundaries) => match boundaries.get(file) {
                Some(Some(size)) if end <= *size => LogBoundary::Existing,
                Some(None) => LogBoundary::Unknown,
                _ => LogBoundary::Appended,
            },
        }
    }

    /// Supplies a process check bracketed by server file inventories.
    pub fn process_checked(
        &mut self,
        present: bool,
        before: Option<&ServerFiles>,
        after: Option<&ServerFiles>,
        log: HubState,
        wall: LogTime,
    ) -> CheckOutcome {
        let recheck = std::mem::take(&mut self.awaiting_recheck);
        if self.untrusted {
            return CheckOutcome::Ignored;
        }
        if present {
            self.last_present = Some(true);
            self.update_starting(log, wall);
            return CheckOutcome::Counted;
        }

        let Some(files) = before.filter(|files| after == Some(*files)) else {
            return CheckOutcome::RetrySoon;
        };
        self.last_present = Some(false);
        self.waiting_for_start = true;
        self.boundaries = Some(
            files
                .iter()
                .map(|(name, &len)| (name.clone(), Some(len)))
                .collect(),
        );
        self.stopped_mark = self.stopped_mark.max(self.latest_applied);
        self.untrusted = recheck;
        self.update_starting(log, wall);
        if recheck {
            CheckOutcome::NameMismatch
        } else {
            CheckOutcome::Counted
        }
    }

    /// Returns whether the caller should check the process again before applying this record.
    pub fn before_record(&mut self, file: &str, end: u64) -> bool {
        let recheck = !self.untrusted
            && self.last_present == Some(false)
            && self.boundary(file, end) == LogBoundary::Appended;
        self.awaiting_recheck = recheck;
        recheck
    }

    /// Reports a server record after applying it. Use the timestamp the engine applied.
    pub fn record_applied(
        &mut self,
        file: &str,
        end: u64,
        at: LogTime,
        event: Option<&Event>,
        log: HubState,
        wall: LogTime,
    ) {
        self.latest_applied = self.latest_applied.max(Some(at));
        let boundary = self.boundary(file, end);
        if boundary == LogBoundary::Existing {
            self.stopped_mark = self.stopped_mark.max(Some(at));
        }
        if matches!(event, Some(Event::ServerStarted))
            && (boundary == LogBoundary::Appended
                || (boundary == LogBoundary::Unknown
                    && self.stopped_mark.is_none_or(|mark| at > mark)))
        {
            self.waiting_for_start = false;
        }
        self.update_starting(log, wall);
    }

    /// Invalidates only the boundary of a replaced server file.
    pub fn rebuild(&mut self, replaced_server_file: Option<&str>) {
        if let (Some(boundaries), Some(file)) = (&mut self.boundaries, replaced_server_file) {
            boundaries.insert(file.to_owned(), None);
        }
    }

    pub fn starting_since(&self) -> Option<LogTime> {
        self.starting_since
    }

    /// Reports a start wait that is strictly longer than the configured limit.
    pub fn start_wait_exceeded(&self, log: HubState, wall: LogTime, timeout: Duration) -> bool {
        self.live(log) == HubLive::Starting
            && self.starting_since.is_some_and(|since| {
                i128::from(wall.0) - i128::from(since.0)
                    > i128::try_from(timeout.as_millis()).unwrap_or(i128::MAX)
            })
    }

    fn update_starting(&mut self, log: HubState, wall: LogTime) {
        if self.live(log) == HubLive::Starting {
            self.starting_since.get_or_insert(wall);
        } else {
            self.starting_since = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "ViveTrackerServer_20261128.txt";
    const B: &str = "ViveTrackerServer_20261127.txt";
    const C: &str = "ViveTrackerServer_20261129.txt";
    const RUNNING: HubState = HubState::Running;

    fn files(a: u64) -> ServerFiles {
        [(A.to_owned(), a), (B.to_owned(), 500)].into()
    }

    fn missing(gate: &mut HubGate, files: &ServerFiles, wall: i64) -> CheckOutcome {
        gate.process_checked(false, Some(files), Some(files), RUNNING, LogTime(wall))
    }

    fn found(gate: &mut HubGate, wall: i64) {
        assert_eq!(
            gate.process_checked(true, None, None, RUNNING, LogTime(wall)),
            CheckOutcome::Counted,
        );
    }

    fn row(gate: &mut HubGate, file: &str, end: u64, at: i64, start: bool) {
        let event = start.then_some(Event::ServerStarted);
        gate.record_applied(
            file,
            end,
            LogTime(at),
            event.as_ref(),
            RUNNING,
            LogTime(201),
        );
    }

    #[test]
    fn only_stable_inventories_confirm_process_absence() {
        let mut gate = HubGate::default();
        let before = files(1000);
        let larger = files(1001);
        let renamed = [(C.to_owned(), 1_000), (B.to_owned(), 500)].into();
        assert_eq!(gate.live(RUNNING), HubLive::Unknown);
        assert!(!gate.before_record(A, 1200));
        for (first, last) in [
            (Some(&before), Some(&larger)),
            (Some(&before), Some(&renamed)),
            (Some(&before), None),
            (None, Some(&before)),
            (None, None),
        ] {
            assert_eq!(
                gate.process_checked(false, first, last, RUNNING, LogTime(0)),
                CheckOutcome::RetrySoon,
            );
            assert_eq!(gate.live(RUNNING), HubLive::Unknown);
            assert_eq!(gate.boundary(A, 1000), LogBoundary::Unmonitored);
        }
        assert_eq!(
            gate.process_checked(true, Some(&before), Some(&larger), RUNNING, LogTime(1)),
            CheckOutcome::Counted,
        );
        assert_eq!(gate.live(RUNNING), HubLive::Running);
        assert!(!gate.before_record(A, 1200));
        assert_eq!(missing(&mut gate, &before, 2), CheckOutcome::Counted);
        assert_eq!(gate.live(RUNNING), HubLive::Stopped);
    }

    #[test]
    fn old_records_keep_start_pending_and_appended_start_does_not_use_the_old_time() {
        let mut gate = HubGate::default();
        let files = files(1000);
        row(&mut gate, A, 970, 100, false);
        assert_eq!(missing(&mut gate, &files, 100), CheckOutcome::Counted);
        for (file, end, expected) in [
            (A, 980, LogBoundary::Existing),
            (A, 1000, LogBoundary::Existing),
            (A, 1001, LogBoundary::Appended),
            (B, 500, LogBoundary::Existing),
            (C, 1, LogBoundary::Appended),
        ] {
            assert_eq!(gate.boundary(file, end), expected);
        }
        assert_eq!(missing(&mut gate, &files, 101), CheckOutcome::Counted);
        assert!(!gate.before_record(A, 980));
        assert!(!gate.before_record(A, 1000));
        found(&mut gate, 200);
        assert_eq!(gate.live(RUNNING), HubLive::Starting);
        assert!(!gate.before_record(A, 1001));
        row(&mut gate, A, 980, 102, true);
        row(&mut gate, A, 1000, 109, false);
        assert_eq!(gate.live(RUNNING), HubLive::Starting);
        assert_eq!(gate.starting_since(), Some(LogTime(200)));
        row(&mut gate, A, 1200, 105, true);
        assert_eq!(gate.live(RUNNING), HubLive::Running);
        assert_eq!(gate.starting_since(), None);
    }

    #[test]
    fn immediate_recheck_handles_found_stable_absent_and_inconclusive_results() {
        let files_a = files(1000);
        for expected in [
            CheckOutcome::Counted,
            CheckOutcome::RetrySoon,
            CheckOutcome::NameMismatch,
        ] {
            let mut gate = HubGate::default();
            assert_eq!(missing(&mut gate, &files_a, 0), CheckOutcome::Counted);
            assert!(!gate.before_record(A, 1000));
            assert!(gate.before_record(A, 1001));
            let after = files_a.clone();
            let next = if expected == CheckOutcome::RetrySoon {
                files(1201)
            } else {
                after.clone()
            };
            assert_eq!(
                gate.process_checked(
                    expected == CheckOutcome::Counted,
                    Some(&after),
                    Some(&next),
                    RUNNING,
                    LogTime(10),
                ),
                expected,
            );
            match expected {
                CheckOutcome::Counted => assert_eq!(gate.live(RUNNING), HubLive::Starting),
                CheckOutcome::RetrySoon => assert_eq!(gate.live(RUNNING), HubLive::Stopped),
                CheckOutcome::NameMismatch => assert_eq!(gate.live(RUNNING), HubLive::Unknown),
                CheckOutcome::Ignored => unreachable!(),
            }
            row(&mut gate, A, 1001, 12, true);
            if expected == CheckOutcome::NameMismatch {
                assert!(!gate.before_record(A, 2000));
                assert_eq!(missing(&mut gate, &files_a, 20), CheckOutcome::Ignored);
                assert_eq!(
                    gate.process_checked(true, None, None, RUNNING, LogTime(21)),
                    CheckOutcome::Ignored,
                );
                assert_eq!(gate.live(RUNNING), HubLive::Unknown);
            } else {
                if expected == CheckOutcome::RetrySoon {
                    found(&mut gate, 20);
                }
                assert_eq!(gate.live(RUNNING), HubLive::Running);
            }
        }
    }

    #[test]
    fn startup_absence_uses_the_inventory_even_before_any_record_is_applied() {
        let mut gate = HubGate::default();
        let files = files(1_000);
        missing(&mut gate, &files, 0);
        assert!(!gate.before_record(B, 300));
        row(&mut gate, B, 300, 45, true);
        assert!(!gate.before_record(A, 1_000));
        row(&mut gate, A, 1_000, 50, true);
        assert_eq!(gate.live(RUNNING), HubLive::Stopped);
        assert!(gate.before_record(A, 1_001));
        found(&mut gate, 60);
        assert_eq!(gate.live(RUNNING), HubLive::Starting);
        row(&mut gate, A, 1_001, 61, true);
        assert_eq!(gate.live(RUNNING), HubLive::Running);

        let mut empty = HubGate::default();
        missing(&mut empty, &ServerFiles::new(), 0);
        assert!(empty.before_record(A, 1));
        found(&mut empty, 1);
        row(&mut empty, A, 1, 2, true);
        assert_eq!(empty.live(RUNNING), HubLive::Running);
    }

    #[test]
    fn rebuild_preserves_stopped_evidence_and_invalidates_only_the_replaced_boundary() {
        let mut gate = HubGate::default();
        let old = files(1_000);
        missing(&mut gate, &old, 100);
        row(&mut gate, A, 1_000, 109, false);
        found(&mut gate, 200);
        gate.rebuild(None);
        assert_eq!(gate.live(HubState::Unknown), HubLive::Starting);
        row(&mut gate, A, 980, 102, true);
        assert_eq!(gate.live(RUNNING), HubLive::Starting);

        gate.rebuild(Some(B));
        assert_eq!(gate.boundary(A, 980), LogBoundary::Existing);
        assert_eq!(gate.boundary(B, 5), LogBoundary::Unknown);
        gate.rebuild(Some(A));
        assert_eq!(gate.boundary(A, 980), LogBoundary::Unknown);
        row(&mut gate, A, 2, 108, true);
        row(&mut gate, A, 4, 109, true);
        assert_eq!(gate.live(RUNNING), HubLive::Starting);
        row(&mut gate, A, 6, 110, true);
        assert_eq!(gate.live(RUNNING), HubLive::Running);

        missing(&mut gate, &old, 230);
        gate.rebuild(Some(A));
        assert!(!gate.before_record(A, 1500));
        missing(&mut gate, &files(400), 231);
        assert_eq!(gate.boundary(A, 400), LogBoundary::Existing);
        assert_eq!(gate.boundary(A, 401), LogBoundary::Appended);
    }

    #[test]
    fn a_start_wait_expires_after_the_limit_even_if_a_file_is_rebuilt() {
        let mut gate = HubGate::default();
        missing(&mut gate, &files(1000), 0);
        found(&mut gate, 200);
        row(&mut gate, A, 980, 100, true);
        gate.rebuild(None);
        let timeout = Duration::from_secs(60);
        assert!(!gate.start_wait_exceeded(RUNNING, LogTime(60_200), timeout));
        assert!(gate.start_wait_exceeded(RUNNING, LogTime(60_201), timeout));
        row(&mut gate, A, 1_001, 200, true);
        assert!(!gate.start_wait_exceeded(RUNNING, LogTime(61_000), timeout));
    }

    #[test]
    fn log_stopped_with_a_present_process_means_starting() {
        let mut gate = HubGate::default();
        gate.process_checked(true, None, None, HubState::Stopped, LogTime(35));
        assert_eq!(gate.live(HubState::Stopped), HubLive::Starting);
        assert_eq!(gate.starting_since(), Some(LogTime(35)));
        row(&mut gate, A, 5, 50, true);
        assert_eq!(gate.live(RUNNING), HubLive::Running);
        assert_eq!(gate.starting_since(), None);
    }

    #[test]
    fn output_is_held_while_stopped_or_starting_and_without_a_process_check() {
        for live in [
            HubLive::Unknown,
            HubLive::Running,
            HubLive::Stopped,
            HubLive::Starting,
        ] {
            for log in [HubState::Unknown, HubState::Running, HubState::Stopped] {
                let expected = match live {
                    HubLive::Unknown => log == HubState::Stopped,
                    HubLive::Running => false,
                    _ => true,
                };
                assert_eq!(hub_down(live, log), expected);
            }
        }
    }
}
