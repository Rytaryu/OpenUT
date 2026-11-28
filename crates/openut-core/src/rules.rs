//! Tracker detection rules and their configurable thresholds.

mod r1;
mod r4;
mod tracking;

use crate::event::Timed;
use crate::state::{ConnId, ConnectionChange, State};
use crate::time::{LogTime, UtcOffset};
use crate::tracker::{Serial, TrackerKey};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RuleId {
    R1,
    R2,
    R3,
    R4,
    R5,
}

impl RuleId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R1 => "R1",
            Self::R2 => "R2",
            Self::R3 => "R3",
            Self::R4 => "R4",
            Self::R5 => "R5",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Raised,
    Escalated,
    Resolved,
    Ended,
}

/// One step of a rule's episode, with the target as identified at that step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Detection {
    pub rule: RuleId,
    pub tracker: Option<TrackerKey>,
    pub episode: u64,
    pub serial: Option<Serial>,
    pub conn: Option<ConnId>,
    /// Whether a host Wi-Fi query had been seen at the latest R1 alert.
    pub host_wifi_queried: Option<bool>,
    pub at: LogTime,
    pub step: Step,
}

pub(crate) struct PowerOffCut {
    pub at: LogTime,
    pub offset: UtcOffset,
    pub tracker: TrackerKey,
    pub conn: Option<ConnId>,
    pub serial: Option<Serial>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RulesConfig {
    pub r1: R1Config,
    pub r2: R2Config,
    pub r3: R3Config,
    pub r4: R4Config,
    pub r5: R5Config,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct R1Config {
    pub count: u64,
    pub span_s: u64,
    pub gap_s: u64,
    pub escalate_s: u64,
    pub quiet_s: u64,
}

impl Default for R1Config {
    fn default() -> Self {
        Self {
            count: 3,
            span_s: 15,
            gap_s: 10,
            escalate_s: 180,
            quiet_s: 60,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct R2Config {
    pub wait_s: u64,
}

impl Default for R2Config {
    fn default() -> Self {
        Self { wait_s: 30 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct R3Config {
    pub wait_s: u64,
    pub clients: bool,
}

impl Default for R3Config {
    fn default() -> Self {
        Self {
            wait_s: 60,
            clients: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct R4Config {
    pub cmd_window_s: u64,
    pub min_connected_s: u64,
    pub burst_s: u64,
    pub server_s: u64,
    pub exclude_power_off: bool,
    pub notice_before_s: u64,
    pub notice_after_s: u64,
    pub back_s: u64,
}

impl Default for R4Config {
    fn default() -> Self {
        Self {
            cmd_window_s: 12,
            min_connected_s: 30,
            burst_s: 2,
            server_s: 5,
            exclude_power_off: true,
            notice_before_s: 15,
            notice_after_s: 1,
            back_s: 600,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct R5Config {
    pub wait_s: u64,
    pub enabled: bool,
}

impl Default for R5Config {
    fn default() -> Self {
        Self {
            wait_s: 60,
            enabled: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Target {
    tracker: Option<TrackerKey>,
    serial: Option<Serial>,
    conn: Option<ConnId>,
}

impl Target {
    fn detection(self, rule: RuleId, episode: u64, at: LogTime, step: Step) -> Detection {
        Detection {
            rule,
            tracker: self.tracker,
            episode,
            serial: self.serial,
            conn: self.conn,
            host_wifi_queried: None,
            at,
            step,
        }
    }
}

pub(crate) fn after(at: LogTime, seconds: u64) -> LogTime {
    let millis = i128::from(at.0) + i128::from(seconds) * 1_000;
    LogTime(i64::try_from(millis).unwrap_or(i64::MAX))
}

fn new_episode(next: &mut u64) -> u64 {
    *next = next.saturating_add(1);
    *next
}

#[derive(Default)]
pub(crate) struct Rules {
    r1: r1::R1,
    r4: r4::R4,
    tracking: tracking::Tracking,
    next_episode: u64,
}

type Timer = (LogTime, RuleId, Option<TrackerKey>);

impl Rules {
    pub(crate) fn apply(
        &mut self,
        ev: &Timed,
        changes: &[ConnectionChange],
        state: &State,
        cfg: &RulesConfig,
        output: &mut Vec<Detection>,
    ) {
        self.r1
            .apply(ev, state, &cfg.r1, output, &mut self.next_episode);
        self.tracking.apply(ev, state, cfg, output);
        self.r4.apply(ev, changes, &cfg.r4, output);
    }

    pub(crate) fn remember_driver_event(
        &mut self,
        ev: &Timed,
        cfg: &RulesConfig,
        output: &mut Vec<Detection>,
    ) {
        self.r4.apply(ev, &[], &cfg.r4, output);
    }

    fn earliest(&self) -> Option<Timer> {
        [
            self.r1.deadline().map(|at| (at, RuleId::R1, None)),
            self.tracking
                .deadline()
                .map(|(at, rule, key)| (at, rule, Some(key))),
            self.r4
                .deadline()
                .map(|(at, key)| (at, RuleId::R4, Some(key))),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(crate) fn next_deadline(&self) -> Option<LogTime> {
        self.earliest().map(|(at, _, _)| at)
    }

    pub(crate) fn r4_deadline(&self) -> Option<LogTime> {
        self.r4.decision_deadline()
    }

    /// Runs timers strictly earlier than the next input, preserving deadline order.
    pub(crate) fn advance(
        &mut self,
        to: LogTime,
        state: &State,
        cfg: &RulesConfig,
        output: &mut Vec<Detection>,
    ) -> Vec<PowerOffCut> {
        let mut power_offs = Vec::new();
        while let Some((at, rule, _)) = self.earliest().filter(|(at, _, _)| *at < to) {
            match rule {
                RuleId::R1 => self.r1.fire(at, state, output),
                RuleId::R4 => {
                    if let Some(cut) = self.r4.fire(at, &cfg.r4, output, &mut self.next_episode) {
                        power_offs.push(cut);
                    }
                }
                RuleId::R2 | RuleId::R3 | RuleId::R5 => {
                    self.tracking
                        .fire(at, state, output, &mut self.next_episode);
                }
            }
        }
        power_offs
    }
}
