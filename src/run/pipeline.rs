//! Transform read-only tracker reports into the local SteamVR bridge stream.

use std::{fmt, num::NonZeroU32};

use openut_bridge::{DeviceEntry, Packet, Payload, Pose as BridgePose, TrackingState};
use openut_dongle::decode::{Decoder, DongleEvent, PoseSample};
use openut_dongle::record::{RawReport, TrackerLabel};
use openut_dongle::tracker::{Link, TrackerTable, Transition};
use openut_space::{AxisMap, BodyRotation, HeldPose, Pose, PoseHold, Quat, Vec3};

const DEVICE_LIST_INTERVAL_US: u64 = 1_000_000;
const DONGLE_SLOTS: u8 = 5;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PipelineStats {
    pub device_lists: u64,
    pub poses: u64,
    pub malformed_reports: u64,
    pub unknown_slot_poses: u64,
    pub conflicting_slot_poses: u64,
    pub invalid_pose_samples: u64,
}

/// Only the non-identifying fields required by the console monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackerStatus {
    pub label: TrackerLabel,
    pub link: Link,
    pub receiving_data: bool,
    pub slot: Option<u8>,
    pub tracking_status: Option<i8>,
    pub pose_rate_hz: f32,
}

impl fmt::Display for TrackerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.label.0.is_ascii_uppercase() {
            self.label.0
        } else {
            '?'
        };
        let reception = match self.link {
            Link::Receiving if self.receiving_data => "受信中",
            Link::Receiving => "接続",
            Link::Silent => "受信が途切れています",
            Link::Unpaired => "未接続",
            Link::NotSeen => "未受信",
        };
        write!(f, "トラッカー {label}　")?;
        if let Some(slot) = self.slot {
            write!(f, "枠 {slot}　")?;
        }
        let tracking = match self.tracking_status {
            Some(2) => "追跡中",
            Some(3 | 4) => "向きのみ",
            Some(-1..=1) => "追跡待ち",
            _ => "追跡状態不明",
        };
        write!(
            f,
            "{reception}　{tracking}　姿勢 {:.0} 件/秒",
            self.pose_rate_hz
        )
    }
}

struct Device {
    slot: u8,
    tracker: TrackerLabel,
    hold: PoseHold,
}

pub struct Pipeline {
    session: NonZeroU32,
    seq: u32,
    decoder: Decoder,
    table: TrackerTable,
    axis_map: AxisMap,
    body_rotation: BodyRotation,
    devices: Vec<Device>,
    last_list_tick_us: u64,
    stats: PipelineStats,
}

impl Pipeline {
    pub fn new(
        session: NonZeroU32,
        start_us: u64,
        axis_map: AxisMap,
        body_rotation: BodyRotation,
    ) -> Self {
        let mut table = TrackerTable::default();
        table.advance(start_us);
        Self {
            session,
            seq: 0,
            decoder: Decoder::default(),
            table,
            axis_map,
            body_rotation,
            devices: Vec::new(),
            last_list_tick_us: start_us,
            stats: PipelineStats::default(),
        }
    }

    pub fn report(&mut self, report: &RawReport, sending_us: u64) -> Vec<Packet> {
        let event = self.decoder.decode(&report.bytes);
        self.apply_event(report.mono_us, sending_us, &event)
    }

    /// Accepts an event already decoded by `openut_dongle::replay`.
    pub fn apply_event(
        &mut self,
        received_us: u64,
        sending_us: u64,
        event: &DongleEvent,
    ) -> Vec<Packet> {
        let changes = self.table.apply(received_us, event);
        self.reset_unpaired(&changes);
        let previously_known = self.devices.len();
        let pose = match event {
            DongleEvent::Pose { tracker, sample } => {
                self.pose(*tracker, sample, sending_us.saturating_sub(received_us))
            }
            DongleEvent::Malformed { .. } => {
                self.stats.malformed_reports = self.stats.malformed_reports.saturating_add(1);
                None
            }
            _ => None,
        };

        let list = self.device_list(self.devices.len() != previously_known);
        let pose = pose.map(|pose| self.packet(Payload::Pose(pose)));
        list.into_iter().chain(pose).collect()
    }

    /// A redacted record can still identify an incoming tracker report.
    pub fn receive_omitted(&mut self, at_us: u64, label: TrackerLabel) -> Vec<Packet> {
        self.table.receive_omitted(at_us, label);
        self.device_list(false).into_iter().collect()
    }

    pub fn advance_to(&mut self, now_us: u64) -> Vec<Packet> {
        self.table.advance(now_us);
        self.device_list(false).into_iter().collect()
    }

    pub fn stats(&self) -> PipelineStats {
        self.stats
    }

    pub fn status(&self) -> Vec<TrackerStatus> {
        self.table
            .views()
            .map(|view| TrackerStatus {
                label: view.label,
                link: view.link,
                receiving_data: view.receiving_data,
                slot: view
                    .last_pose
                    .as_ref()
                    .map(|sample| sample.slot)
                    .filter(|slot| *slot < DONGLE_SLOTS),
                tracking_status: view.tracking_status,
                pose_rate_hz: view.pose_rate_hz,
            })
            .collect()
    }

    fn reset_unpaired(&mut self, changes: &[Transition]) {
        for change in changes.iter().filter(|change| change.to == Link::Unpaired) {
            for device in self
                .devices
                .iter_mut()
                .filter(|device| device.tracker == change.tracker)
            {
                device.hold = PoseHold::default();
            }
        }
    }

    fn pose(
        &mut self,
        tracker: TrackerLabel,
        sample: &PoseSample,
        age_us: u64,
    ) -> Option<BridgePose> {
        if sample.slot >= DONGLE_SLOTS {
            self.stats.unknown_slot_poses = self.stats.unknown_slot_poses.saturating_add(1);
            return None;
        }
        let device = self.device(tracker, sample.slot)?;
        let mut pose = BridgePose {
            device,
            state: TrackingState::Invalid,
            age_us: u32::try_from(age_us).unwrap_or(u32::MAX),
            position: [0.0; 3],
            rotation: [1.0, 0.0, 0.0, 0.0],
            velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        };
        if matches!(sample.tracking_status, 2..=4) {
            if let Some(converted) = convert(sample, self.axis_map, self.body_rotation) {
                let held = self.devices[usize::from(device)]
                    .hold
                    .apply(sample.tracking_status, converted);
                match held {
                    HeldPose::Tracked(value) | HeldPose::RotationOnly(value) => {
                        pose.state = if matches!(held, HeldPose::Tracked(_)) {
                            TrackingState::Tracked
                        } else {
                            TrackingState::RotationOnly
                        };
                        pose.position = [
                            value.position.x as f32,
                            value.position.y as f32,
                            value.position.z as f32,
                        ];
                        pose.rotation = [
                            value.rotation.w as f32,
                            value.rotation.x as f32,
                            value.rotation.y as f32,
                            value.rotation.z as f32,
                        ];
                    }
                    HeldPose::Invalid => {}
                }
            } else {
                self.stats.invalid_pose_samples = self.stats.invalid_pose_samples.saturating_add(1);
            }
        }
        self.stats.poses = self.stats.poses.saturating_add(1);
        Some(pose)
    }

    fn device(&mut self, tracker: TrackerLabel, slot: u8) -> Option<u8> {
        if let Some((index, device)) = self
            .devices
            .iter_mut()
            .enumerate()
            .find(|(_, device)| device.slot == slot)
        {
            if device.tracker != tracker {
                let slot_in_use = self.table.view(device.tracker).is_some_and(|view| {
                    view.link == Link::Receiving
                        && view
                            .last_pose
                            .as_ref()
                            .is_some_and(|pose| pose.slot == slot)
                });
                if slot_in_use {
                    self.stats.conflicting_slot_poses =
                        self.stats.conflicting_slot_poses.saturating_add(1);
                    return None;
                }
                device.tracker = tracker;
                device.hold = PoseHold::default();
            }
            return Some(index as u8);
        }

        let index = self.devices.len() as u8;
        self.devices.push(Device {
            slot,
            tracker,
            hold: PoseHold::default(),
        });
        Some(index)
    }

    fn device_list(&mut self, grew: bool) -> Option<Packet> {
        let now = self.table.now_us();
        let elapsed = now.saturating_sub(self.last_list_tick_us);
        let passed_ticks = elapsed / DEVICE_LIST_INTERVAL_US;
        if !grew && passed_ticks == 0 {
            return None;
        }
        // Skip missed ticks without sending a burst of redundant lists.
        self.last_list_tick_us += passed_ticks * DEVICE_LIST_INTERVAL_US;
        self.stats.device_lists = self.stats.device_lists.saturating_add(1);
        let entries = self
            .devices
            .iter()
            .enumerate()
            .map(|(id, device)| DeviceEntry {
                device: id as u8,
                name: format!("OpenUT-slot{}", device.slot),
            })
            .collect();
        Some(self.packet(Payload::DeviceList(entries)))
    }

    fn packet(&mut self, payload: Payload) -> Packet {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        Packet {
            session: self.session.get(),
            seq,
            payload,
        }
    }
}

fn convert(sample: &PoseSample, map: AxisMap, body: BodyRotation) -> Option<Pose> {
    let [w, x, y, z] = sample.rotation_wxyz.map(f64::from);
    let rotation = Quat::new(w, x, y, z).normalized()?;
    let position = if sample.tracking_status == 2 {
        let [x, y, z] = sample.position_m.map(f64::from);
        if ![x, y, z].into_iter().all(f64::is_finite) {
            return None;
        }
        map.apply_position(Vec3::new(x, y, z))
    } else {
        // A rotation-only report uses the last tracked position instead.
        Vec3::ZERO
    };
    Some(Pose {
        position,
        rotation: body.apply_rotation(map.apply_rotation(rotation)),
    })
}
