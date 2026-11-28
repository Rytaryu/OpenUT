//! Tracker state derived from a stream of decoded reports and caller-supplied times.

use std::collections::{BTreeMap, VecDeque};

use crate::decode::{DeviceInfoField, DongleEvent, PoseSample, Redactable, TrackerMessage};
use crate::record::TrackerLabel;

const SILENCE_US: u64 = 2_000_000;
const POSE_WINDOW_US: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Link {
    NotSeen,
    Receiving,
    Silent,
    Unpaired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transition {
    pub at_us: u64,
    pub tracker: TrackerLabel,
    pub from: Link,
    pub to: Link,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackerView {
    pub label: TrackerLabel,
    pub link: Link,
    /// Whether a data report has arrived during the current receiving interval.
    pub receiving_data: bool,
    pub tracking_status: Option<i8>,
    pub slam_tracking_status: Option<i64>,
    pub map_state: Option<i32>,
    pub slam_map_state: Option<i64>,
    pub is_host: Option<bool>,
    pub role_id: Option<String>,
    pub wifi_connected_to_host: Option<bool>,
    pub firmware: Option<String>,
    pub last_pose: Option<PoseSample>,
    pub pose_rate_hz: f32,
    pub last_error: Option<u32>,
    /// Most recent tracker data report. A pairing notice can start a link before data arrives.
    pub last_seen_us: u64,
}

#[derive(Default)]
pub struct TrackerTable {
    trackers: BTreeMap<char, TrackerState>,
    now_us: u64,
    time_reversed: u64,
}

impl TrackerTable {
    /// Applies a report after advancing to its time. Pass reports and time markers in stream order.
    pub fn apply(&mut self, mono_us: u64, event: &DongleEvent) -> Vec<Transition> {
        let mut transitions = self.advance(mono_us);
        let now_us = self.now_us;
        match event {
            DongleEvent::Pair { tracker, unpaired } => {
                let link = if *unpaired {
                    Link::Unpaired
                } else {
                    Link::Receiving
                };
                self.receive(*tracker, link, &mut transitions);
            }
            DongleEvent::Pose { tracker, sample } => {
                self.receive_data(*tracker, Link::Receiving, &mut transitions)
                    .add_pose(now_us, sample);
            }
            DongleEvent::Text { tracker, message } => {
                let link = match message {
                    TrackerMessage::PowerOff | TrackerMessage::Reset => Link::Unpaired,
                    _ => Link::Receiving,
                };
                self.receive_data(*tracker, link, &mut transitions)
                    .update_text(message);
            }
            DongleEvent::OtherTrackerData { tracker, .. } => {
                self.receive_data(*tracker, Link::Receiving, &mut transitions);
            }
            DongleEvent::RfStatus { .. }
            | DongleEvent::Unknown { .. }
            | DongleEvent::Malformed { .. } => {}
        }
        transitions
    }

    /// Accounts for tracker data whose body was omitted from an anonymized recording.
    pub fn receive_omitted(&mut self, mono_us: u64, label: TrackerLabel) -> Vec<Transition> {
        let mut transitions = self.advance(mono_us);
        self.receive_data(label, Link::Receiving, &mut transitions);
        transitions
    }

    /// Advances by stream time, emitting expired links at their deadline.
    pub fn advance(&mut self, now_us: u64) -> Vec<Transition> {
        if now_us < self.now_us {
            self.time_reversed = self.time_reversed.saturating_add(1);
        }
        self.now_us = self.now_us.max(now_us);

        let mut transitions = Vec::new();
        for tracker in self.trackers.values_mut() {
            tracker.age_poses(self.now_us);
            if tracker.view.link == Link::Receiving
                && self.now_us - tracker.last_activity_us >= SILENCE_US
            {
                tracker.set_link(
                    tracker.last_activity_us + SILENCE_US,
                    Link::Silent,
                    &mut transitions,
                );
            }
        }
        // A single timestamp can pass multiple deadlines. Emit the same order as fine-grained ticks.
        transitions.sort_unstable_by_key(|change| (change.at_us, change.tracker.0));
        transitions
    }

    pub fn time_reversed(&self) -> u64 {
        self.time_reversed
    }

    pub fn now_us(&self) -> u64 {
        self.now_us
    }

    pub fn views(&self) -> impl Iterator<Item = &TrackerView> {
        self.trackers.values().map(|tracker| &tracker.view)
    }

    pub fn view(&self, label: TrackerLabel) -> Option<&TrackerView> {
        self.trackers.get(&label.0).map(|tracker| &tracker.view)
    }

    fn receive_data(
        &mut self,
        label: TrackerLabel,
        link: Link,
        transitions: &mut Vec<Transition>,
    ) -> &mut TrackerState {
        let now_us = self.now_us;
        let tracker = self.receive(label, link, transitions);
        tracker.view.receiving_data = link == Link::Receiving;
        tracker.view.last_seen_us = now_us;
        tracker
    }

    fn receive(
        &mut self,
        label: TrackerLabel,
        link: Link,
        transitions: &mut Vec<Transition>,
    ) -> &mut TrackerState {
        let tracker = self
            .trackers
            .entry(label.0)
            .or_insert_with(|| TrackerState::new(label));
        tracker.last_activity_us = self.now_us;
        tracker.set_link(self.now_us, link, transitions);
        tracker
    }
}

struct TrackerState {
    view: TrackerView,
    last_activity_us: u64,
    pose_times: VecDeque<u64>,
}

impl TrackerState {
    fn new(label: TrackerLabel) -> Self {
        Self {
            view: TrackerView {
                label,
                link: Link::NotSeen,
                receiving_data: false,
                tracking_status: None,
                slam_tracking_status: None,
                map_state: None,
                slam_map_state: None,
                is_host: None,
                role_id: None,
                wifi_connected_to_host: None,
                firmware: None,
                last_pose: None,
                pose_rate_hz: 0.0,
                last_error: None,
                last_seen_us: 0,
            },
            last_activity_us: 0,
            pose_times: VecDeque::new(),
        }
    }

    fn set_link(&mut self, at_us: u64, to: Link, transitions: &mut Vec<Transition>) {
        let from = self.view.link;
        if from != to {
            transitions.push(Transition {
                at_us,
                tracker: self.view.label,
                from,
                to,
            });
            self.view.link = to;
            self.view.receiving_data = false;
        }
    }

    fn add_pose(&mut self, now_us: u64, sample: &PoseSample) {
        self.view.last_pose = Some(sample.clone());
        self.view.tracking_status = Some(sample.tracking_status);
        self.pose_times.push_back(now_us);
        self.view.pose_rate_hz = self.pose_times.len() as f32;
    }

    fn age_poses(&mut self, now_us: u64) {
        while self
            .pose_times
            .front()
            .is_some_and(|at_us| now_us - at_us >= POSE_WINDOW_US)
        {
            self.pose_times.pop_front();
        }
        self.view.pose_rate_hz = self.pose_times.len() as f32;
    }

    fn update_text(&mut self, message: &TrackerMessage) {
        match message {
            TrackerMessage::MapStatus { b, .. } => self.view.map_state = Some(*b),
            TrackerMessage::SlamStatus { key: 5, value } => {
                self.view.slam_map_state = Some(*value);
            }
            TrackerMessage::SlamStatus { key: 6, value } => {
                self.view.slam_tracking_status = Some(*value);
            }
            TrackerMessage::WifiHostInfo => {
                self.view.is_host = Some(true);
                self.view.wifi_connected_to_host = None;
            }
            TrackerMessage::WifiInfoRequest => {
                self.view.is_host = Some(false);
                self.view.wifi_connected_to_host = Some(false);
            }
            TrackerMessage::WifiConnected { ok } => {
                self.view.is_host = Some(false);
                self.view.wifi_connected_to_host = Some(*ok);
            }
            TrackerMessage::DeviceInfo {
                field: DeviceInfoField::Nari,
                value,
            } => self.view.role_id = safe_role_id(value).map(str::to_owned),
            TrackerMessage::DeviceInfo {
                field: DeviceInfoField::Nav1,
                value,
            } => self.view.firmware = safe_firmware(value).map(str::to_owned),
            TrackerMessage::SlamError { code } => self.view.last_error = Some(*code),
            _ => {}
        }
    }
}

pub(crate) fn safe_role_id(value: &Redactable) -> Option<&str> {
    let value = value.expose();
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    (!digits.is_empty() && digits.len() <= 10 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(value)
}

pub(crate) fn safe_firmware(value: &Redactable) -> Option<&str> {
    let value = value.expose();
    (value.len() <= 32
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.'))
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Decoder;

    #[test]
    fn retains_map_and_tracking_sources_and_only_validated_device_info() {
        let mut table = TrackerTable::default();
        let mut decoder = Decoder::default();
        for (now_us, message) in [
            b"MS0,3".as_slice(),
            b"P61:5,6",
            b"P61:6,2",
            b"WS",
            b"WC1",
            b"NARI+42",
            b"NAV11.2.3",
            b"NADSsynthetic-private-sample",
            b"P82:0:1100",
        ]
        .into_iter()
        .enumerate()
        {
            let mut report = vec![0x28, 0, 0, 1, 1, 2, 3, 4, 5, 0x01, 0x01];
            report.push(u8::try_from(message.len()).expect("synthetic message is short"));
            report.extend_from_slice(message);
            table.apply(now_us as u64, &decoder.decode(&report));
        }

        let state = table.view(TrackerLabel('A')).expect("tracker A");
        assert_eq!(state.map_state, Some(3));
        assert_eq!(state.slam_map_state, Some(6));
        assert_eq!(state.slam_tracking_status, Some(2));
        assert_eq!(state.role_id.as_deref(), Some("+42"));
        assert_eq!(state.firmware.as_deref(), Some("1.2.3"));
        assert_eq!(state.is_host, Some(false));
        assert_eq!(state.wifi_connected_to_host, Some(true));
        assert_eq!(state.last_error, Some(1100));
        assert!(!format!("{state:?}").contains("synthetic-private-sample"));
    }

    #[test]
    fn ages_the_pose_count_at_the_one_second_boundary() {
        let mut table = TrackerTable::default();
        let sample = PoseSample {
            slot: 0,
            buttons_raw: 0,
            position_m: [0.0; 3],
            rotation_wxyz: [1.0, 0.0, 0.0, 0.0],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 4],
            tracking_status: 3,
            extra: Vec::new(),
        };
        let tracker = TrackerLabel('A');
        table.apply(2_000_000, &DongleEvent::Pose { tracker, sample });
        table.advance(2_999_999);
        assert_eq!(table.view(tracker).expect("tracker A").pose_rate_hz, 1.0);
        table.advance(3_000_000);
        let state = table.view(tracker).expect("tracker A");
        assert_eq!(state.pose_rate_hz, 0.0);
        assert_eq!(state.tracking_status, Some(3));
        assert_eq!(state.link, Link::Receiving);
    }

    #[test]
    fn distinguishes_pairing_from_data_even_at_time_zero() {
        let mut table = TrackerTable::default();
        let tracker = TrackerLabel('A');
        let pair = DongleEvent::Pair {
            tracker,
            unpaired: false,
        };
        table.apply(0, &pair);
        let view = table.view(tracker).expect("tracker A");
        assert!(!view.receiving_data);
        assert!(crate::view::tracker_line(view, None).contains("つながった"));

        table.receive_omitted(0, tracker);
        let view = table.view(tracker).expect("tracker A");
        assert!(view.receiving_data);
        assert!(crate::view::tracker_line(view, None).contains("受信中"));

        table.advance(2_000_000);
        table.apply(2_000_000, &pair);
        assert!(!table.view(tracker).expect("tracker A").receiving_data);
    }
}
