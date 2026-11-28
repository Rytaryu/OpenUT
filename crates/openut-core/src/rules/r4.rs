//! Unexpected tracker disconnections (watcher design §6.2, R4).

use std::collections::{BTreeMap, BTreeSet};

use crate::event::{Event, Timed};
use crate::state::{ConnectionChange, ConnectionChangeKind, DisconnectReason};
use crate::time::LogTime;
use crate::tracker::{Serial, TrackerKey};

use super::{Detection, PowerOffCut, R4Config, RuleId, Step, Target, after, new_episode};

#[derive(Default)]
pub(super) struct R4 {
    starts: BTreeMap<TrackerKey, ConnectionStart>,
    ends: BTreeMap<TrackerKey, LogTime>,
    commands: BTreeMap<Option<TrackerKey>, LogTime>,
    removals: BTreeMap<TrackerKey, LogTime>,
    disrupted_at: Option<LogTime>,
    notices: BTreeMap<TrackerKey, BTreeSet<LogTime>>,
    pending: BTreeMap<TimerKey, Pending>,
    raised: BTreeMap<TimerKey, Raised>,
    next_timer_order: u64,
}

type TimerKey = (LogTime, TrackerKey, u64);

struct ConnectionStart {
    at: LogTime,
    shares_end_at: bool,
}

struct Pending {
    start: ConnectionStart,
    cut_at: LogTime,
    next_start: Option<LogTime>,
    target: Target,
    back_at: LogTime,
    recovered: bool,
}

struct Raised {
    target: Target,
    episode: u64,
}

#[derive(Clone, Copy)]
enum Recovery<'a> {
    TrackerAdded(TrackerKey),
    SerialKnown(&'a Serial),
}

impl Recovery<'_> {
    fn matches(self, target: &Target) -> bool {
        match self {
            Self::TrackerAdded(tracker) => {
                target.serial.is_none() && target.tracker == Some(tracker)
            }
            Self::SerialKnown(serial) => target.serial.as_ref() == Some(serial),
        }
    }
}

impl R4 {
    pub(super) fn apply(
        &mut self,
        ev: &Timed,
        changes: &[ConnectionChange],
        cfg: &R4Config,
        output: &mut Vec<Detection>,
    ) {
        let at = ev.at;
        match &ev.event {
            Event::PowerOffNotice { tracker } => {
                if cfg.exclude_power_off {
                    self.notices.entry(*tracker).or_default().insert(at);
                }
                return;
            }
            Event::PcCommand { tracker, .. } => self.command(*tracker, at),
            Event::HostAssigned { tracker } | Event::WifiAssigned { tracker } => {
                self.command(Some(*tracker), at);
            }
            Event::HostWifiQueried { host } => self.command(Some(*host), at),
            Event::ServerStarted | Event::ServerStopped | Event::DongleRemoved => {
                self.disrupted_at = Some(at);
                self.pending
                    .retain(|_, cut| at > after(cut.cut_at, cfg.server_s));
            }
            Event::TrackerRemoved { tracker, .. } => {
                self.removals.insert(*tracker, at);
                self.pending.retain(|&(_, key, _), cut| {
                    key == *tracker || at > after(cut.cut_at, cfg.burst_s)
                });
            }
            _ => {}
        }

        for change in changes {
            let key = change.tracker.key;
            match change.kind {
                ConnectionChangeKind::Started => {
                    self.connect(key, change.at);
                    if matches!(ev.event, Event::TrackerAdded { tracker, .. } if tracker == key) {
                        self.recover(Recovery::TrackerAdded(key), at, output);
                    }
                }
                ConnectionChangeKind::Ended(reason) => {
                    let start = self.starts.remove(&key);
                    self.ends.insert(key, change.at);
                    if reason == DisconnectReason::TrackerRemoved
                        && matches!(ev.event, Event::TrackerRemoved { usb: false, .. })
                        && let Some(start) = start
                    {
                        self.consider_cut(change, start, cfg);
                    }
                }
            }
        }
        if let Event::SerialKnown {
            tracker, serial, ..
        } = &ev.event
            && self.starts.contains_key(tracker)
        {
            self.recover(Recovery::SerialKnown(serial), at, output);
        }
        self.prune_notices(at, cfg);
    }

    fn command(&mut self, key: Option<TrackerKey>, at: LogTime) {
        self.commands.insert(key, at);
        // A command can follow a removal at the same timestamp in the file.
        self.pending.retain(|&(_, tracker, _), cut| {
            cut.cut_at != at || key.is_some_and(|key| key != tracker)
        });
    }

    fn connect(&mut self, key: TrackerKey, at: LogTime) {
        self.starts.insert(
            key,
            ConnectionStart {
                at,
                shares_end_at: self.ends.get(&key) == Some(&at),
            },
        );
        for (&(_, tracker, _), cut) in &mut self.pending {
            if tracker == key {
                cut.next_start.get_or_insert(at);
            }
        }
    }

    fn consider_cut(&mut self, change: &ConnectionChange, start: ConnectionStart, cfg: &R4Config) {
        let at = change.at;
        let tracker = change.tracker.key;
        let command_from = before(at, cfg.cmd_window_s);
        let recent_command = [None, Some(tracker)].into_iter().any(|key| {
            self.commands
                .get(&key)
                .is_some_and(|&ts| ts >= command_from)
        });
        let recent_disruption = self
            .disrupted_at
            .is_some_and(|ts| ts >= before(at, cfg.server_s));
        let burst_from = before(at, cfg.burst_s);
        let nearby_removal = self
            .removals
            .iter()
            .any(|(&key, &ts)| key != tracker && ts >= burst_from);
        if at < after(start.at, cfg.min_connected_s)
            || recent_command
            || recent_disruption
            || nearby_removal
        {
            return;
        }

        let deadline = after(at, cfg.burst_s.max(cfg.server_s).max(cfg.notice_after_s));
        let timer = self.timer_key(deadline, tracker);
        self.pending.insert(
            timer,
            Pending {
                start,
                cut_at: at,
                next_start: None,
                target: Target {
                    tracker: Some(tracker),
                    serial: change.tracker.serial.clone(),
                    conn: Some(change.tracker.conn),
                },
                back_at: after(at, cfg.back_s),
                recovered: false,
            },
        );
    }

    fn prune_notices(&mut self, now: LogTime, cfg: &R4Config) {
        let oldest_cut = self
            .pending
            .values()
            .map(|cut| cut.cut_at)
            .min()
            .unwrap_or(now);
        let oldest_notice = before(oldest_cut, cfg.notice_before_s);
        self.notices.retain(|_, times| {
            while times.first().is_some_and(|&time| time < oldest_notice) {
                times.pop_first();
            }
            !times.is_empty()
        });
    }

    fn recover(&mut self, evidence: Recovery<'_>, at: LogTime, output: &mut Vec<Detection>) {
        for cut in self.pending.values_mut() {
            if at <= cut.back_at && evidence.matches(&cut.target) {
                cut.recovered = true;
            }
        }
        self.raised.retain(|&(back_at, _, _), raised| {
            if at <= back_at && evidence.matches(&raised.target) {
                output.push(raised.target.clone().detection(
                    RuleId::R4,
                    raised.episode,
                    at,
                    Step::Resolved,
                ));
                false
            } else {
                true
            }
        });
    }

    pub(super) fn decision_deadline(&self) -> Option<LogTime> {
        self.pending.first_key_value().map(|(key, _)| key.0)
    }

    pub(super) fn deadline(&self) -> Option<(LogTime, TrackerKey)> {
        self.next_timer()
            .map(|((deadline, tracker, _), _)| (deadline, tracker))
    }

    fn next_timer(&self) -> Option<(TimerKey, bool)> {
        let pending = self.pending.first_key_value().map(|(&key, _)| key);
        let raised = self.raised.first_key_value().map(|(&key, _)| key);
        match (pending, raised) {
            (Some(pending), Some(raised)) if raised < pending => Some((raised, false)),
            (Some(pending), _) => Some((pending, true)),
            (None, raised) => raised.map(|key| (key, false)),
        }
    }

    fn timer_key(&mut self, at: LogTime, tracker: TrackerKey) -> TimerKey {
        self.next_timer_order = self.next_timer_order.saturating_add(1);
        (at, tracker, self.next_timer_order)
    }

    pub(super) fn fire(
        &mut self,
        at: LogTime,
        cfg: &R4Config,
        output: &mut Vec<Detection>,
        next_episode: &mut u64,
    ) -> Option<PowerOffCut> {
        let ((deadline, tracker, _), decide) = self.next_timer()?;
        debug_assert_eq!(at, deadline);
        if !decide {
            let (_, raised) = self.raised.pop_first()?;
            output.push(
                raised
                    .target
                    .detection(RuleId::R4, raised.episode, deadline, Step::Ended),
            );
            return None;
        }

        let (_, cut) = self.pending.pop_first()?;
        if cfg.exclude_power_off && self.has_notice(tracker, &cut, cfg) {
            return Some(PowerOffCut {
                at: cut.cut_at,
                tracker,
                conn: cut.target.conn,
                serial: cut.target.serial,
            });
        }

        let episode = new_episode(next_episode);
        output.push(
            cut.target
                .clone()
                .detection(RuleId::R4, episode, deadline, Step::Raised),
        );
        if cut.recovered {
            // A short disconnection still counts, but has already recovered.
            output.push(
                cut.target
                    .detection(RuleId::R4, episode, deadline, Step::Resolved),
            );
        } else {
            let timer = self.timer_key(cut.back_at.max(deadline), tracker);
            self.raised.insert(
                timer,
                Raised {
                    target: cut.target,
                    episode,
                },
            );
        }
        None
    }

    fn has_notice(&self, tracker: TrackerKey, cut: &Pending, cfg: &R4Config) -> bool {
        let first = before(cut.cut_at, cfg.notice_before_s).max(cut.start.at);
        let last = after(cut.cut_at, cfg.notice_after_s);
        self.notices.get(&tracker).is_some_and(|times| {
            times.range(first..=last).any(|&time| {
                // At a shared boundary the notice belongs to the connection ending there.
                !(cut.start.shares_end_at && time == cut.start.at)
                    && (time <= cut.cut_at || cut.next_start.is_none_or(|next| time < next))
            })
        })
    }
}

fn before(at: LogTime, seconds: u64) -> LogTime {
    let millis = i128::from(at.0) - i128::from(seconds) * 1_000;
    LogTime(i64::try_from(millis).unwrap_or(i64::MIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::state::{ConnId, State};
    use crate::time::UtcOffset;

    const CUT: i64 = 40_000;
    const DECISION: i64 = CUT + 5_000;
    const A: TrackerKey = TrackerKey(1);
    const B: TrackerKey = TrackerKey(2);

    #[derive(Default)]
    struct Scenario {
        rule: R4,
        state: State,
        cfg: R4Config,
        output: Vec<Detection>,
        power_offs: Vec<LogTime>,
        next_episode: u64,
    }

    impl Scenario {
        fn advance(&mut self, at: i64) {
            while let Some((deadline, _)) = self.rule.deadline().filter(|(ts, _)| ts.0 < at) {
                if let Some(cut) = self.rule.fire(
                    deadline,
                    &self.cfg,
                    &mut self.output,
                    &mut self.next_episode,
                ) {
                    self.power_offs.push(cut.at);
                }
            }
        }

        fn event(&mut self, at: i64, event: Event) {
            let ev = Timed {
                at: LogTime(at),
                offset: UtcOffset(0),
                event,
            };
            let changes = if matches!(ev.event, Event::PowerOffNotice { .. }) {
                Vec::new()
            } else {
                self.advance(at);
                self.state.apply(&ev).connections
            };
            self.rule.apply(&ev, &changes, &self.cfg, &mut self.output);
        }

        fn add(&mut self, at: i64, tracker: TrackerKey) {
            self.event(
                at,
                Event::TrackerAdded {
                    tracker,
                    usb: false,
                },
            );
        }

        fn cut(&mut self, at: i64, tracker: TrackerKey) {
            self.event(
                at,
                Event::TrackerRemoved {
                    tracker,
                    usb: false,
                },
            );
        }

        fn notice(&mut self, at: i64, tracker: TrackerKey) {
            self.event(at, Event::PowerOffNotice { tracker });
        }

        fn serial(&mut self, at: i64, tracker: TrackerKey, serial: &Serial) {
            self.event(
                at,
                Event::SerialKnown {
                    tracker,
                    serial: serial.clone(),
                    hub_number: None,
                    firmware: None,
                },
            );
        }

        fn steps(&self) -> Vec<(LogTime, Step)> {
            self.output
                .iter()
                .map(|detection| (detection.at, detection.step))
                .collect()
        }
    }

    #[test]
    fn duration_and_command_windows_include_their_boundaries() {
        for (start, command_at, command_target, after_cut, raises) in [
            (10_000, CUT - 12_000, None, false, false),
            (10_000, CUT - 12_001, Some(A), false, true),
            (10_001, CUT - 12_001, None, false, false),
            (10_000, CUT, Some(A), true, false),
            (10_000, CUT, Some(B), true, true),
        ] {
            let mut case = Scenario::default();
            case.add(start, A);
            let command = Event::PcCommand {
                tracker: command_target,
                kind: String::new(),
            };
            if after_cut {
                case.cut(CUT, A);
                case.event(command_at, command);
            } else {
                case.event(command_at, command);
                case.cut(CUT, A);
            }
            case.advance(DECISION);
            assert!(case.output.is_empty(), "deadline itself stays pending");
            case.advance(DECISION + 1);
            assert_eq!(case.output.len(), usize::from(raises));
            if raises {
                assert_eq!(case.steps(), [(LogTime(DECISION), Step::Raised)]);
            }
        }
    }

    #[test]
    fn same_slot_deadlines_tie_in_timer_creation_order() {
        let mut case = Scenario::default();
        case.cfg.min_connected_s = 1;
        case.cfg.back_s = 10;
        case.add(0, A);
        case.serial(1, A, &Serial("SYNTHETIC-A".into()));
        case.cut(CUT, A);
        case.add(CUT + 1_000, A);
        case.serial(CUT + 1_001, A, &Serial("SYNTHETIC-B".into()));
        case.cut(DECISION, A);

        case.advance(CUT + 10_001);
        assert_eq!(
            case.steps(),
            [
                (LogTime(DECISION), Step::Raised),
                (LogTime(CUT + 10_000), Step::Raised),
                (LogTime(CUT + 10_000), Step::Ended),
            ]
        );
        assert_eq!(case.output[0].episode, case.output[2].episode);
    }

    #[test]
    fn adjacent_removals_disruptions_and_usb_exclude_the_cut() {
        for (event, when, raises) in [
            (
                Event::TrackerRemoved {
                    tracker: B,
                    usb: false,
                },
                CUT - 2_000,
                false,
            ),
            (
                Event::TrackerRemoved {
                    tracker: B,
                    usb: false,
                },
                CUT + 2_000,
                false,
            ),
            (
                Event::TrackerRemoved {
                    tracker: B,
                    usb: false,
                },
                CUT + 2_001,
                true,
            ),
            (Event::ServerStarted, DECISION, false),
            (Event::ServerStarted, DECISION + 1, true),
            (Event::ServerStopped, DECISION, false),
            (Event::DongleRemoved, DECISION, false),
        ] {
            let mut case = Scenario::default();
            case.add(0, A);
            if when < CUT {
                case.event(when, event);
                case.cut(CUT, A);
            } else {
                case.cut(CUT, A);
                case.event(when, event);
            }
            case.advance(DECISION + 1);
            assert_eq!(case.output.len(), usize::from(raises));
        }

        let mut usb = Scenario::default();
        usb.add(0, A);
        usb.event(
            CUT,
            Event::TrackerRemoved {
                tracker: A,
                usb: true,
            },
        );
        usb.advance(DECISION + 1);
        assert!(usb.output.is_empty());
    }

    #[test]
    fn power_off_notice_windows_include_both_edges() {
        for (notice, exclude, raises) in [
            (CUT - 15_000, true, false),
            (CUT - 15_001, true, true),
            (CUT + 1_000, true, false),
            (CUT + 1_001, true, true),
            (CUT, false, true),
        ] {
            let mut case = Scenario::default();
            case.cfg.exclude_power_off = exclude;
            case.add(0, A);
            case.cut(CUT, A);
            case.cut(CUT + 3, A);
            assert_eq!(case.rule.decision_deadline(), Some(LogTime(DECISION)));
            case.notice(notice, A);
            case.advance(DECISION + 1);
            assert_eq!(case.output.len(), usize::from(raises));
            assert_eq!(
                case.power_offs,
                (!raises)
                    .then_some(LogTime(CUT))
                    .into_iter()
                    .collect::<Vec<_>>(),
            );
            assert_eq!(case.rule.decision_deadline(), None);
        }
    }

    #[test]
    fn late_or_same_instant_notices_use_connection_intervals() {
        for (next_start, notice, notice_after_s, raises) in [
            (CUT + 3_000, CUT + 8_000, 10, true),
            (CUT + 500, CUT + 800, 1, true),
            (CUT + 3_000, CUT, 1, false),
            (CUT, CUT, 1, false),
        ] {
            let mut case = Scenario::default();
            case.cfg.notice_after_s = notice_after_s;
            case.add(0, A);
            case.cut(CUT, A);
            case.add(next_start, A);
            case.notice(notice, A);
            case.advance(CUT + (notice_after_s.max(5) * 1_000) as i64 + 1);
            assert_eq!(
                case.output.iter().any(|row| row.step == Step::Raised),
                raises
            );
            assert_eq!(
                case.power_offs,
                (!raises)
                    .then_some(LogTime(CUT))
                    .into_iter()
                    .collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn reconnect_restarts_connection_length_and_keeps_previous_notices_separate() {
        for (duration, notice, minimum, raises) in [
            (3_000, None, 30, false),
            (40_000, None, 30, true),
            (12_000, Some(CUT), 10, true),
            (12_000, Some(CUT + 5_000), 10, false),
        ] {
            let mut case = Scenario::default();
            case.cfg.min_connected_s = minimum;
            case.add(0, A);
            case.add(CUT, A);
            assert_eq!(
                case.rule.decision_deadline(),
                None,
                "reconnect is not a removal"
            );
            case.cut(CUT + duration, A);
            if let Some(notice) = notice {
                case.notice(notice, A);
            }
            case.advance(CUT + duration + 5_001);
            assert_eq!(case.output.len(), usize::from(raises));
        }

        let mut previous = Scenario::default();
        previous.cfg.min_connected_s = 10;
        previous.add(0, A);
        previous.cut(CUT - 14_000, A);
        previous.notice(CUT - 14_000, A);
        previous.add(CUT - 13_000, A);
        previous.cut(CUT, A);
        previous.advance(DECISION + 1);
        assert_eq!(previous.power_offs, [LogTime(CUT - 14_000)]);
        assert_eq!(previous.steps(), [(LogTime(DECISION), Step::Raised)]);
    }

    #[test]
    fn recognition_by_serial_resolves_the_original_connection() {
        let mut case = Scenario::default();
        let serial = Serial("SYNTHETIC-TRACKER-A".to_owned());
        case.add(0, A);
        case.serial(1, A, &serial);
        case.cut(CUT, A);
        case.cut(CUT + 3, A);
        case.advance(DECISION + 1);
        case.add(DECISION + 2, A);
        assert_eq!(case.steps(), [(LogTime(DECISION), Step::Raised)]);
        case.add(DECISION + 3, B);
        case.serial(DECISION + 4, B, &serial);
        let [raised, resolved] = case.output.as_slice() else {
            panic!("expected an episode and its resolution");
        };
        assert_eq!(resolved.step, Step::Resolved);
        assert_eq!(resolved.at, LogTime(DECISION + 4));
        assert_eq!(resolved.episode, raised.episode);
        assert_eq!(resolved.tracker, Some(A));
        assert_eq!(resolved.conn, Some(ConnId(1)));
        assert_eq!(resolved.serial.as_ref(), Some(&serial));
    }

    #[test]
    fn a_cut_that_recovers_before_decision_still_counts() {
        let mut recovered = Scenario::default();
        recovered.add(0, A);
        recovered.cut(CUT, A);
        recovered.add(CUT + 100, A);
        recovered.advance(DECISION + 1);
        assert_eq!(
            recovered.steps(),
            [
                (LogTime(DECISION), Step::Raised),
                (LogTime(DECISION), Step::Resolved)
            ],
        );
        assert_eq!(recovered.rule.deadline(), None);

        let mut expired = Scenario::default();
        expired.cfg.back_s = 8;
        expired.add(0, A);
        expired.cut(CUT, A);
        expired.advance(CUT + 8_001);
        assert_eq!(
            expired.steps(),
            [
                (LogTime(DECISION), Step::Raised),
                (LogTime(CUT + 8_000), Step::Ended)
            ],
        );
    }
}
