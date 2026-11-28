//! Owns registration and tracking state. The frame runner executes returned actions.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use openut_bridge::{DeviceEntry, MAX_DEVICES, Packet, Payload, Pose as BridgePose};

use crate::pose::Pose;

const DEFAULT_POSE_TIMEOUT: Duration = Duration::from_millis(500);
const DEFAULT_LIST_TIMEOUT: Duration = Duration::from_secs(3);
// The localhost receive queue can briefly contain packets from a previous app run.
const RETIRED_SESSION_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    Activated { slot: usize, index: u32 },
    ActivationFailed { slot: usize },
    Deactivated { slot: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Add { slot: usize, name: String },
    Pose { slot: usize, index: u32, pose: Pose },
    Log { message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Registration {
    Unregistered,
    Waiting,
    Active(u32),
    Rejected,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Published {
    Sample,
    Stale,
    Disconnected,
}

#[derive(Clone, Copy)]
struct Sample {
    pose: BridgePose,
    received_at: Instant,
}

struct Tracker {
    name: String,
    registration: Registration,
    latest: Option<Sample>,
    published: Option<Published>,
}

impl Tracker {
    fn new(name: String) -> Self {
        Self {
            name,
            registration: Registration::Unregistered,
            latest: None,
            published: None,
        }
    }

    fn publish(
        &mut self,
        slot: usize,
        at: Instant,
        list_fresh: bool,
        pose_timeout: Duration,
        actions: &mut Vec<Action>,
    ) {
        let Registration::Active(index) = self.registration else {
            return;
        };
        let latest = self
            .latest
            .filter(|sample| recent(at, sample.received_at, pose_timeout));
        let (published, pose) = match (list_fresh, latest) {
            (false, _) => (Published::Disconnected, Pose::invalid(false)),
            (true, None) => (Published::Stale, Pose::invalid(true)),
            (true, Some(sample)) => (
                Published::Sample,
                Pose::from_packet(
                    sample.pose,
                    at.saturating_duration_since(sample.received_at),
                ),
            ),
        };
        if self.published != Some(published) {
            self.published = Some(published);
            actions.push(Action::Pose { slot, index, pose });
        }
    }
}

fn recent(at: Instant, received: Instant, timeout: Duration) -> bool {
    at.saturating_duration_since(received) <= timeout
}

struct Session {
    id: u32,
    slots: [Option<usize>; MAX_DEVICES],
    last_pose_seq: Option<u32>,
    last_list_seq: Option<u32>,
}

impl Session {
    fn new(id: u32) -> Self {
        Self {
            id,
            slots: [None; MAX_DEVICES],
            last_pose_seq: None,
            last_list_seq: None,
        }
    }
}

pub struct Table {
    trackers: Vec<Tracker>,
    session: Option<Session>,
    retired: VecDeque<u32>,
    last_list: Option<Instant>,
    pose_timeout: Duration,
    list_timeout: Duration,
    dropped: u64,
}

impl Default for Table {
    fn default() -> Self {
        Self::new(DEFAULT_POSE_TIMEOUT, DEFAULT_LIST_TIMEOUT)
    }
}

impl Table {
    pub fn new(pose_timeout: Duration, list_timeout: Duration) -> Self {
        Self {
            trackers: Vec::with_capacity(MAX_DEVICES),
            session: None,
            retired: VecDeque::with_capacity(RETIRED_SESSION_LIMIT),
            last_list: None,
            pose_timeout,
            list_timeout,
            dropped: 0,
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Reports the host's boolean registration result. Only Activate supplies an index.
    pub fn on_added(&mut self, slot: usize, accepted: bool) -> Option<String> {
        let tracker = self.trackers.get_mut(slot)?;
        if tracker.registration != Registration::Unregistered {
            return None;
        }
        if accepted {
            tracker.registration = Registration::Waiting;
            None
        } else {
            tracker.registration = Registration::Rejected;
            tracker.latest = None;
            Some(format!("openut: SteamVR refused tracker slot {slot}"))
        }
    }

    pub fn on_callback(&mut self, callback: Callback, now: Instant, actions: &mut Vec<Action>) {
        let list_fresh = self.list_fresh(now);
        match callback {
            Callback::Activated { slot, index } => {
                let Some(tracker) = self.trackers.get_mut(slot) else {
                    return;
                };
                if !matches!(
                    tracker.registration,
                    Registration::Waiting | Registration::Disabled
                ) {
                    return;
                }
                if tracker.registration == Registration::Disabled {
                    actions.push(Action::Log {
                        message: format!(
                            "openut: reactivated tracker slot {slot} at index {index}"
                        ),
                    });
                }
                tracker.registration = Registration::Active(index);
                tracker.published = None;
                tracker.publish(slot, now, list_fresh, self.pose_timeout, actions);
            }
            Callback::ActivationFailed { slot } => {
                let Some(tracker) = self.trackers.get_mut(slot) else {
                    return;
                };
                if matches!(
                    tracker.registration,
                    Registration::Waiting | Registration::Disabled
                ) {
                    tracker.registration = Registration::Rejected;
                    tracker.latest = None;
                    actions.push(Action::Log {
                        message: format!("openut: activation failed for tracker slot {slot}"),
                    });
                }
            }
            Callback::Deactivated { slot } => {
                let Some(tracker) = self.trackers.get_mut(slot) else {
                    return;
                };
                if tracker.registration != Registration::Rejected {
                    tracker.registration = Registration::Disabled;
                    tracker.latest = None;
                }
            }
        }
    }

    pub fn on_packet(
        &mut self,
        packet: Packet,
        received_at: Instant,
        now: Instant,
        actions: &mut Vec<Action>,
    ) {
        if packet.session == 0 || self.retired.contains(&packet.session) {
            self.drop_packet();
            return;
        }
        if self
            .session
            .as_ref()
            .is_none_or(|session| session.id != packet.session)
        {
            self.begin_session(packet.session);
        }
        match packet.payload {
            Payload::DeviceList(devices) => {
                self.device_list(packet.seq, received_at, devices, actions);
            }
            Payload::Pose(pose) => self.pose(packet.seq, received_at, now, pose, actions),
        }
    }

    pub fn finish_frame(&mut self, now: Instant, actions: &mut Vec<Action>) {
        let list_fresh = self.list_fresh(now);
        for (slot, tracker) in self.trackers.iter_mut().enumerate() {
            tracker.publish(slot, now, list_fresh, self.pose_timeout, actions);
        }
    }

    fn begin_session(&mut self, id: u32) {
        if let Some(old) = self.session.replace(Session::new(id)) {
            if self.retired.len() == RETIRED_SESSION_LIMIT {
                self.retired.pop_front();
            }
            self.retired.push_back(old.id);
            // Keep SteamVR registration, but never forward a previous run's pose.
            for tracker in &mut self.trackers {
                tracker.latest = None;
            }
        }
    }

    fn device_list(
        &mut self,
        seq: u32,
        received_at: Instant,
        devices: Vec<DeviceEntry>,
        actions: &mut Vec<Action>,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.last_list_seq.is_some_and(|last_seq| seq < last_seq) {
            self.drop_packet();
            return;
        }
        session.last_list_seq = Some(seq);
        let mut slots = [None; MAX_DEVICES];
        for entry in devices {
            let wire_index = usize::from(entry.device);
            let Some(wire_slot) = slots.get(wire_index) else {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            };
            if wire_slot.is_some() {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            }
            let tracker_slot = match self
                .trackers
                .iter()
                .position(|tracker| tracker.name == entry.name)
            {
                Some(slot) => slot,
                None if self.trackers.len() < MAX_DEVICES => {
                    let slot = self.trackers.len();
                    actions.push(Action::Add {
                        slot,
                        name: entry.name.clone(),
                    });
                    self.trackers.push(Tracker::new(entry.name));
                    slot
                }
                None => {
                    self.dropped = self.dropped.saturating_add(1);
                    continue;
                }
            };
            if slots.contains(&Some(tracker_slot)) {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            }
            slots[wire_index] = Some(tracker_slot);
        }
        session.slots = slots;
        self.last_list = Some(received_at);
    }

    fn pose(
        &mut self,
        seq: u32,
        received_at: Instant,
        now: Instant,
        pose: BridgePose,
        actions: &mut Vec<Action>,
    ) {
        let list_fresh = self.list_fresh(now);
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(slot) = session
            .slots
            .get(usize::from(pose.device))
            .copied()
            .flatten()
        else {
            self.drop_packet();
            return;
        };
        if session.last_pose_seq.is_some_and(|last_seq| seq < last_seq) {
            self.drop_packet();
            return;
        }
        session.last_pose_seq = Some(seq);
        let Some(tracker) = self.trackers.get_mut(slot) else {
            self.drop_packet();
            return;
        };
        if matches!(
            tracker.registration,
            Registration::Rejected | Registration::Disabled
        ) {
            self.drop_packet();
            return;
        }
        tracker.latest = Some(Sample { pose, received_at });
        if tracker.published == Some(Published::Sample)
            && list_fresh
            && recent(now, received_at, self.pose_timeout)
        {
            tracker.published = None;
        }
        tracker.publish(slot, now, list_fresh, self.pose_timeout, actions);
    }

    fn list_fresh(&self, at: Instant) -> bool {
        self.last_list
            .is_some_and(|last| recent(at, last, self.list_timeout))
    }

    fn drop_packet(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }
}
