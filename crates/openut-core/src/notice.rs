//! Log-time notice delivery and live notice state (watcher design §7.1).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::rules::{Detection, RuleId, Step, after};
use crate::state::{ConnId, Snapshot};
use crate::text::{add_other_notices, add_repetitions, host_label, notice_text, target_label};
use crate::time::LogTime;
use crate::tracker::{Serial, TrackerKey};

pub use crate::text::{NoticeText, TextPart, TrackerLabel};

#[cfg(test)]
mod tests;

const RECONNECTED_DISPLAY_S: u64 = 30;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoticeConfig {
    pub cooldown_s: u64,
    pub min_gap_s: u64,
}

impl Default for NoticeConfig {
    fn default() -> Self {
        Self {
            cooldown_s: 300,
            min_gap_s: 10,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notice {
    pub rule: RuleId,
    pub tracker: Option<TrackerKey>,
    pub episode: u64,
    pub serial: Option<Serial>,
    pub conn: Option<ConnId>,
    pub step: Step,
    pub at: LogTime,
    pub text: NoticeText,
    pub target: TrackerLabel,
    pub push: bool,
    pub priority: u8,
}

struct Subject {
    rule: RuleId,
    tracker: Option<TrackerKey>,
    serial: Option<Serial>,
}

impl Subject {
    fn of(notice: &Notice) -> Self {
        Self {
            rule: notice.rule,
            tracker: notice.tracker,
            serial: notice.serial.clone(),
        }
    }

    fn matches(&self, notice: &Notice) -> bool {
        self.rule == notice.rule
            && match (&self.serial, &notice.serial) {
                (Some(earlier), Some(current)) => earlier == current,
                _ => self.tracker == notice.tracker,
            }
    }
}

struct Active {
    notice: Notice,
    delivered: Option<Delivered>,
    muted: bool,
}

struct Delivered {
    subject: Subject,
    step: Step,
}

struct CooldownEnd {
    subject: Subject,
    at: LogTime,
    undrained_episode: Option<u64>,
}

struct DisconnectPush {
    subject: Subject,
    at: LogTime,
    repeats: u64,
    last_repeat: Option<LogTime>,
    delivered: bool,
}

pub(crate) struct NoticeScheduler {
    cfg: NoticeConfig,
    r5_wait_s: u64,
    catching_up: bool,
    hub_down: bool,
    active: BTreeMap<u64, Active>,
    reconnected: BTreeMap<u64, (LogTime, Notice)>,
    waiting: BTreeMap<u64, Notice>,
    outbox: Vec<Notice>,
    last_delivered: Option<LogTime>,
    cooldown_ends: Vec<CooldownEnd>,
    disconnect_pushes: Vec<DisconnectPush>,
    rebuilding: Vec<Delivered>,
}

impl NoticeScheduler {
    pub(crate) fn new(cfg: NoticeConfig, r5_wait_s: u64) -> Self {
        Self {
            cfg,
            r5_wait_s,
            catching_up: false,
            hub_down: false,
            active: BTreeMap::new(),
            reconnected: BTreeMap::new(),
            waiting: BTreeMap::new(),
            outbox: Vec::new(),
            last_delivered: None,
            cooldown_ends: Vec::new(),
            disconnect_pushes: Vec::new(),
            rebuilding: Vec::new(),
        }
    }

    pub(crate) fn apply(&mut self, detection: &Detection, snapshot: &Snapshot) {
        if !self.catching_up {
            // A replay can rebuild without reopening catch-up. Its live detections
            // must not leave matching candidates for a later rebuild.
            self.rebuilding.clear();
        }
        let mut notice = Notice {
            rule: detection.rule,
            tracker: detection.tracker,
            episode: detection.episode,
            serial: detection.serial.clone(),
            conn: detection.conn,
            step: detection.step,
            at: detection.at,
            text: notice_text(
                detection.rule,
                detection.step,
                self.r5_wait_s,
                &host_label(snapshot),
            ),
            target: target_label(snapshot, detection),
            push: false,
            priority: priority(detection.rule),
        };
        let allowed = !self.catching_up && !self.hub_down;
        let previous = self.active.remove(&notice.episode);
        let mut push = false;
        match notice.step {
            Step::Raised | Step::Escalated => {
                let muted = self.hub_down || previous.as_ref().is_some_and(|active| active.muted);
                let delivered = previous.and_then(|active| active.delivered);
                push = allowed
                    && !muted
                    && !already_delivered(delivered.as_ref(), notice.step)
                    && self.eligible(&mut notice, false);
                self.active.insert(
                    notice.episode,
                    Active {
                        notice: notice.clone(),
                        delivered,
                        muted,
                    },
                );
            }
            Step::Resolved | Step::Ended => {
                self.waiting.remove(&notice.episode);
                if let Some(active) = previous.filter(|active| !active.muted) {
                    push = allowed && notice.rule == RuleId::R1 && notice.step == Step::Resolved;
                    if !self.catching_up && cooldown_after_end(notice.rule) {
                        self.cooldown_ends.push(CooldownEnd {
                            subject: Subject::of(&active.notice),
                            at: notice.at,
                            undrained_episode: Some(notice.episode),
                        });
                    }
                }
                if notice.rule == RuleId::R4 && notice.step == Step::Resolved {
                    self.reconnected.insert(
                        notice.episode,
                        (after(notice.at, RECONNECTED_DISPLAY_S), notice.clone()),
                    );
                }
            }
        }
        self.emit(notice, push);
    }

    fn eligible(&mut self, notice: &mut Notice, releasing_catch_up: bool) -> bool {
        if notice.step != Step::Raised {
            return notice.rule == RuleId::R1 && notice.step == Step::Escalated;
        }
        let cooldown = self.cfg.cooldown_s;
        if notice.rule == RuleId::R4 {
            if let Some(previous) = self.disconnect_pushes.iter_mut().rev().find(|previous| {
                within_cooldown(previous.at, notice.at, cooldown)
                    && previous.subject.matches(notice)
            }) {
                // Rebuilding can reproduce a disconnect already counted earlier.
                if !releasing_catch_up || previous.last_repeat.is_none_or(|at| at < notice.at) {
                    previous.repeats = previous.repeats.saturating_add(1);
                    previous.last_repeat = Some(notice.at);
                }
                notice.text = notice_text(
                    notice.rule,
                    notice.step,
                    self.r5_wait_s,
                    &TrackerLabel::default(),
                );
                add_repetitions(&mut notice.text, previous.repeats);
                return false;
            }
        } else if cooldown_after_end(notice.rule) {
            return !self.cooldown_ends.iter().any(|previous| {
                within_cooldown(previous.at, notice.at, cooldown)
                    && previous.subject.matches(notice)
            });
        }
        true
    }

    fn emit(&mut self, notice: Notice, push: bool) {
        let gap_elapsed = self
            .last_push()
            .is_none_or(|last| after(last, self.cfg.min_gap_s) <= notice.at);
        if push && self.waiting.is_empty() && gap_elapsed {
            let at = notice.at;
            self.prepare(notice, at, 0);
        } else {
            if push {
                self.waiting.insert(notice.episode, notice.clone());
            }
            self.outbox.push(notice);
        }
    }

    fn last_push(&self) -> Option<LogTime> {
        self.outbox
            .iter()
            .rev()
            .find(|notice| notice.push)
            .map(|notice| notice.at)
            .or(self.last_delivered)
    }

    fn push_deadline(&self) -> Option<LogTime> {
        let first = self.waiting.values().map(|notice| notice.at).min()?;
        Some(
            self.last_push()
                .map_or(first, |last| first.max(after(last, self.cfg.min_gap_s))),
        )
    }

    pub(crate) fn next_deadline(&self) -> Option<LogTime> {
        self.push_deadline()
            .into_iter()
            .chain(self.reconnected.values().map(|(until, _)| *until))
            .min()
    }

    pub(crate) fn advance_to(&mut self, to: LogTime) {
        if let Some(at) = self.push_deadline().filter(|at| *at < to) {
            self.prepare_batch(at);
        }
        self.reconnected.retain(|_, (until, _)| *until > to);
        if !self.catching_up && self.rebuilding.is_empty() {
            let cooldown = self.cfg.cooldown_s;
            self.cooldown_ends
                .retain(|previous| after(previous.at, cooldown) >= to);
            self.disconnect_pushes
                .retain(|previous| after(previous.at, cooldown) >= to);
        }
    }

    fn prepare_batch(&mut self, at: LogTime) {
        let mut waiting: Vec<_> = std::mem::take(&mut self.waiting).into_values().collect();
        waiting.sort_by_key(|notice| (Reverse(notice.priority), notice.at, notice.episode));
        if self.cfg.min_gap_s == 0 {
            for notice in waiting {
                self.prepare(notice, at, 0);
            }
        } else {
            let others = waiting.len().saturating_sub(1);
            if let Some(winner) = waiting.into_iter().next() {
                self.prepare(winner, at, others);
            }
        }
    }

    fn prepare(&mut self, mut notice: Notice, at: LogTime, others: usize) {
        notice.at = at;
        notice.push = true;
        if notice.rule == RuleId::R4 {
            self.disconnect_pushes.push(DisconnectPush {
                subject: Subject::of(&notice),
                at,
                repeats: 1,
                last_repeat: None,
                delivered: false,
            });
        }
        add_other_notices(&mut notice.text, others);
        self.outbox.push(notice);
    }

    pub(crate) fn live(&self) -> Vec<Notice> {
        self.active
            .values()
            .map(|active| active.notice.clone())
            .chain(self.reconnected.values().map(|(_, notice)| notice.clone()))
            .collect()
    }

    pub(crate) fn set_catching_up(&mut self, on: bool, now: LogTime) {
        if self.catching_up == on {
            return;
        }
        self.catching_up = on;
        if on {
            self.cancel_undrained(false);
            return;
        }

        let mut previous = std::mem::take(&mut self.rebuilding);
        let mut active = std::mem::take(&mut self.active);
        for entry in active.values_mut() {
            if let Some((index, _)) = previous
                .iter()
                .enumerate()
                .filter(|(_, push)| push.subject.matches(&entry.notice))
                .max_by_key(|(_, push)| {
                    push.subject.serial.is_some() && entry.notice.serial.is_some()
                })
            {
                entry.delivered = Some(previous.remove(index));
            }
            entry.muted |= self.hub_down;
            if !entry.muted
                && !already_delivered(entry.delivered.as_ref(), entry.notice.step)
                && self.eligible(&mut entry.notice, true)
            {
                let mut notice = entry.notice.clone();
                notice.at = now;
                self.waiting.insert(notice.episode, notice);
            }
        }
        self.active = active;
        if self.push_deadline().is_some_and(|deadline| deadline <= now) {
            self.prepare_batch(now);
        }
    }

    pub(crate) fn set_hub_down(&mut self, on: bool) {
        self.hub_down = on;
        if on {
            self.cancel_undrained(true);
        }
    }

    fn cancel_undrained(&mut self, mute: bool) {
        let mut cancelled: BTreeSet<_> = self.waiting.keys().copied().collect();
        self.waiting.clear();
        for notice in self.outbox.iter_mut().filter(|notice| notice.push) {
            notice.push = false;
            cancelled.insert(notice.episode);
        }
        if mute {
            for episode in &cancelled {
                if let Some(active) = self.active.get_mut(episode) {
                    active.muted = true;
                }
            }
        }
        self.cooldown_ends.retain(|end| {
            end.undrained_episode
                .is_none_or(|episode| !cancelled.contains(&episode))
        });
        self.disconnect_pushes.retain(|push| push.delivered);
    }

    pub(crate) fn rebuild(&mut self) {
        self.cancel_undrained(false);
        self.rebuilding.extend(
            std::mem::take(&mut self.active)
                .into_values()
                .filter_map(|active| active.delivered),
        );
        self.reconnected.clear();
        self.outbox.clear();
    }

    /// Delivery counts only once the caller takes the notice output.
    pub(crate) fn drain(&mut self) -> Vec<Notice> {
        let output = std::mem::take(&mut self.outbox);
        for notice in output.iter().filter(|notice| notice.push) {
            self.last_delivered = Some(notice.at);
            if let Some(active) = self.active.get_mut(&notice.episode) {
                active.delivered = Some(Delivered {
                    subject: Subject::of(notice),
                    step: notice.step,
                });
            }
        }
        for push in &mut self.disconnect_pushes {
            push.delivered = true;
        }
        for end in &mut self.cooldown_ends {
            end.undrained_episode = None;
        }
        output
    }
}

fn already_delivered(delivered: Option<&Delivered>, step: Step) -> bool {
    delivered.is_some_and(|push| push.step == step || push.step == Step::Escalated)
}

fn within_cooldown(previous: LogTime, current: LogTime, seconds: u64) -> bool {
    previous <= current && current < after(previous, seconds)
}

fn cooldown_after_end(rule: RuleId) -> bool {
    matches!(rule, RuleId::R2 | RuleId::R3 | RuleId::R5)
}

fn priority(rule: RuleId) -> u8 {
    match rule {
        RuleId::R1 => 5,
        RuleId::R4 => 4,
        RuleId::R3 => 3,
        RuleId::R2 => 2,
        RuleId::R5 => 1,
    }
}
