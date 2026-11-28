use openut_bridge::{DeviceEntry, Payload, encode};

use super::*;

const TEST_LIMIT: Duration = Duration::from_secs(3);

fn packet(seq: u32) -> Packet {
    Packet {
        session: 1,
        seq,
        payload: Payload::DeviceList(vec![DeviceEntry {
            device: 0,
            name: String::from("Mock tracker"),
        }]),
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < TEST_LIMIT,
            "receiver did not become ready"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn invalid_and_non_local_datagrams_are_counted_and_ignored() {
    let inbox = Inbox::default();
    let now = Instant::now();
    let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 123));
    let foreign = SocketAddr::from(([192, 0, 2, 10], 123));
    let packet = packet(7);
    let data = encode(&packet).unwrap();
    inbox.accept(foreign, &data, now);
    inbox.accept(local, b"bad packet", now);
    inbox.accept(local, &[0; MAX_PACKET_LEN + 1], now);
    inbox.accept(local, &data, now);

    assert_eq!(
        inbox.take_packets(),
        [ReceivedPacket {
            packet,
            received_at: now
        }]
    );
    assert_eq!(inbox.stats().invalid_packets, 2);
    assert_eq!(inbox.stats().foreign_packets, 1);
}

#[test]
fn bounded_inboxes_keep_the_newest_packets_and_log_lines() {
    let inbox = Inbox::default();
    let now = Instant::now();
    for seq in 0..PACKET_CAPACITY + 5 {
        inbox.push_packet(ReceivedPacket {
            packet: packet(seq as u32),
            received_at: now,
        });
    }
    for line in 0..LOG_CAPACITY + 7 {
        inbox.push_log(line.to_string());
    }

    let packets = inbox.take_packets();
    assert_eq!(packets.len(), PACKET_CAPACITY);
    assert_eq!(packets.front().unwrap().packet.seq, 5);
    assert_eq!(
        packets.back().unwrap().packet.seq,
        (PACKET_CAPACITY + 4) as u32
    );
    let logs = inbox.take_logs();
    assert_eq!(logs.len(), LOG_CAPACITY);
    assert_eq!(logs.front().unwrap(), "7");
    assert_eq!(logs.back().unwrap(), &(LOG_CAPACITY + 6).to_string());
    assert_eq!(inbox.stats().dropped_packets, 5);
    assert_eq!(inbox.stats().dropped_logs, 7);
    assert!(inbox.take_packets().is_empty());
    assert!(inbox.take_logs().is_empty());
}

#[test]
fn a_receiver_drains_local_udp_even_when_the_inbox_is_full_and_joins() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    let socket = bind(0).unwrap();
    let addr = socket.local_addr().unwrap();
    assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
    let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let inbox = Arc::new(Inbox::default());
    let thread_inbox = Arc::clone(&inbox);
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(Arc::clone(&dropped));
    let worker = Worker::spawn_task(move |stop| {
        let _guard = guard;
        receive(&socket, stop, &thread_inbox).unwrap();
    })
    .unwrap();

    let now = Instant::now();
    for seq in 0..PACKET_CAPACITY {
        inbox.push_packet(ReceivedPacket {
            packet: packet(seq as u32),
            received_at: now,
        });
    }
    client
        .send_to(&encode(&packet(9_000)).unwrap(), addr)
        .unwrap();
    wait_until(|| inbox.stats().dropped_packets == 1);
    let start = Instant::now();
    worker.stop_and_join();
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(dropped.load(Ordering::Relaxed));
    let packets = inbox.take_packets();
    assert_eq!(packets.front().unwrap().packet.seq, 1);
    assert_eq!(packets.back().unwrap().packet.seq, 9_000);
}

#[test]
fn the_worker_reports_a_busy_port_and_stops_during_bind_retry() {
    let occupied = bind(0).unwrap();
    let port = occupied.local_addr().unwrap().port();
    let inbox = Arc::new(Inbox::default());
    let worker = Worker::spawn(port, Arc::clone(&inbox)).unwrap();
    wait_until(|| inbox.stats().bind_failures == 1);
    let start = Instant::now();
    worker.stop_and_join();
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(Arc::strong_count(&inbox), 1);
    let lines = inbox.take_logs();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("bind failed"));
    assert!(!lines[0].contains(&port.to_string()));
}
