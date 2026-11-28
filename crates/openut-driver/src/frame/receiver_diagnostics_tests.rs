use openut_bridge::{Packet, Payload, Pose as BridgePose, TrackingState};

use crate::receiver::{LOG_CAPACITY, PACKET_CAPACITY, ReceivedPacket};

use super::*;

const REPORT_PREFIX: &str = "openut: receiver queue ";

#[derive(Default)]
struct LogHost(Mutex<Vec<String>>);

impl LogHost {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.0))
    }
}

impl HostApi for LogHost {
    fn add_tracker(&self, _: &FrameOpen, _: &CStr, _: &Device) -> bool {
        panic!("diagnostics must not register a tracker");
    }

    fn update_pose(&self, _: &CheckedIndex, _: &Pose) {
        panic!("diagnostics must not publish a pose");
    }

    fn property_container(&self, _: &FrameOpen, _: u32) -> Option<u64> {
        panic!("diagnostics must not request a property container");
    }

    fn write_properties(&self, _: &FrameOpen, _: u64) -> bool {
        panic!("diagnostics must not write tracker properties");
    }

    fn log(&self, _: &FrameOpen, line: &str) {
        lock(&self.0).push(line.to_owned());
    }

    fn bridge_port(&self, _: &FrameOpen) -> u16 {
        panic!("diagnostics do not need to start the receiver thread");
    }
}

fn overflow_packets(inbox: &Inbox, now: Instant) {
    for seq in 0..=PACKET_CAPACITY as u32 {
        let pose = BridgePose {
            device: 0,
            state: TrackingState::Tracked,
            age_us: 0,
            position: [0.0; 3],
            rotation: [1.0, 0.0, 0.0, 0.0],
            velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        };
        inbox.push_packet(ReceivedPacket {
            received_at: now,
            packet: Packet {
                session: 1,
                seq,
                payload: Payload::Pose(pose),
            },
        });
    }
}

#[test]
fn receiver_overflow_is_visible_in_the_frame_log_and_updates_are_bounded() {
    let frame = Frame::new();
    let host = Arc::new(LogHost::default());
    *lock(&frame.host) = Some(host.clone());
    let inbox = lock(&frame.inbox).clone().unwrap();
    let start = Instant::now();
    let overflow_at = start + Duration::from_millis(1);

    frame.run_frame(start);
    assert_eq!(
        host.take(),
        ["openut: receiver queue dropped_packets=0 dropped_logs=0"]
    );

    overflow_packets(&inbox, overflow_at);
    let retained = inbox.take_packets();
    assert_eq!(retained.len(), PACKET_CAPACITY);
    assert_eq!(retained.front().unwrap().packet.seq, 1);
    for packet in retained {
        inbox.push_packet(packet);
    }
    for index in 0..=LOG_CAPACITY {
        inbox.push_log(format!("openut: test entry {index}"));
    }
    frame.run_frame(overflow_at);
    let lines = host.take();
    assert_eq!(lines.len(), LOG_CAPACITY + 1);
    assert_eq!(lines[0], "openut: test entry 1");
    assert_eq!(
        lines.last().unwrap(),
        "openut: receiver queue dropped_packets=1 dropped_logs=1"
    );
    assert_eq!(inbox.stats().dropped_packets, 1);
    assert_eq!(inbox.stats().dropped_logs, 1);

    let next_overflow_at = overflow_at + Duration::from_millis(1);
    overflow_packets(&inbox, next_overflow_at);
    frame.run_frame(next_overflow_at);
    assert!(host.take().is_empty());

    frame.run_frame(overflow_at + RECEIVER_REPORT_INTERVAL);
    assert_eq!(
        host.take(),
        ["openut: receiver queue dropped_packets=2 dropped_logs=1"]
    );
    frame.run_frame(overflow_at + RECEIVER_REPORT_INTERVAL * 2);
    assert!(
        host.take()
            .iter()
            .all(|line| !line.starts_with(REPORT_PREFIX))
    );
}
