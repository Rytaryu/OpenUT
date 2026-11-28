use super::*;
use crate::receiver::ReceivedPacket;
use openut_bridge::{DeviceEntry, Packet, Payload, Pose as BridgePose, TrackingState};
use std::sync::Weak;

const TIMEOUT: Duration = Duration::from_secs(2);
const FIRST_INDEX: u32 = 40;

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Settings,
    Add(usize),
    Container(u32),
    Properties(u64),
    Pose(u32, Pose),
    Log(String),
}

type Hook = Arc<dyn Fn(&Call) + Send + Sync>;

struct Host {
    frame: Weak<Frame>,
    activate_on_add: bool,
    calls: Mutex<Vec<Call>>,
    hook: Mutex<Option<Hook>>,
}

impl Host {
    fn note(&self, call: Call) {
        let frame = self.frame.upgrade().expect("frame alive");
        assert!(frame.table.try_lock().is_ok());
        assert!(frame.inbox.try_lock().is_ok());
        assert!(frame.slots.try_lock().is_ok());
        assert!(frame.worker.try_lock().is_ok());
        assert!(frame.callbacks.try_lock().is_ok());
        assert!(frame.host.try_lock().is_ok());
        lock(&self.calls).push(call.clone());
        let hook = lock(&self.hook).clone();
        if let Some(hook) = hook {
            hook(&call);
        }
    }

    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut *lock(&self.calls))
    }

    fn hook(&self, hook: impl Fn(&Call) + Send + Sync + 'static) {
        *lock(&self.hook) = Some(Arc::new(hook));
    }
}

impl HostApi for Host {
    fn add_tracker(&self, _: &FrameOpen, _: &CStr, device: &Device) -> bool {
        self.note(Call::Add(device.slot));
        if self.activate_on_add {
            let frame = self.frame.upgrade().expect("frame alive");
            let _ = frame.activate(device, FIRST_INDEX + u32::try_from(device.slot).unwrap());
        }
        true
    }

    fn update_pose(&self, index: &CheckedIndex, pose: &Pose) {
        self.note(Call::Pose(index.get(), *pose));
    }

    fn property_container(&self, _: &FrameOpen, index: u32) -> Option<u64> {
        self.note(Call::Container(index));
        Some(u64::from(index) + 1)
    }

    fn write_properties(&self, _: &FrameOpen, container: u64) -> bool {
        self.note(Call::Properties(container));
        true
    }

    fn log(&self, _: &FrameOpen, line: &str) {
        self.note(Call::Log(line.to_owned()));
    }

    fn bridge_port(&self, _: &FrameOpen) -> u16 {
        self.note(Call::Settings);
        0
    }
}

struct Fixture {
    frame: Arc<Frame>,
    host: Arc<Host>,
    start: Instant,
}

impl Fixture {
    fn new(activate_on_add: bool) -> Self {
        let frame = Arc::new(Frame::new());
        let host = Arc::new(Host {
            frame: Arc::downgrade(&frame),
            activate_on_add,
            calls: Mutex::new(Vec::new()),
            hook: Mutex::new(None),
        });
        assert!(frame.init(host.clone()));
        assert_eq!(host.take(), [Call::Settings]);
        Self {
            frame,
            host,
            start: Instant::now(),
        }
    }

    fn inbox(&self) -> Arc<Inbox> {
        lock(&self.frame.inbox).clone().expect("inbox live")
    }

    fn packet(&self, at_ms: u64, seq: u32, payload: Payload) {
        self.inbox().push_packet(ReceivedPacket {
            packet: Packet {
                session: 1,
                seq,
                payload,
            },
            received_at: self.start + Duration::from_millis(at_ms),
        });
    }

    fn list(&self, seq: u32, names: &[&str]) {
        self.packet(
            0,
            seq,
            Payload::DeviceList(
                names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| DeviceEntry {
                        device: u8::try_from(index).unwrap(),
                        name: (*name).to_owned(),
                    })
                    .collect(),
            ),
        );
    }

    fn pose(&self, seq: u32, device: u8, x: f32) {
        self.packet(
            0,
            seq,
            Payload::Pose(BridgePose {
                device,
                state: TrackingState::Tracked,
                age_us: 0,
                position: [x, 0.0, 0.0],
                rotation: [1.0, 0.0, 0.0, 0.0],
                velocity: [0.0; 3],
                angular_velocity: [0.0; 3],
            }),
        );
    }

    fn run(&self, ms: u64) {
        self.frame.run_frame(self.start + Duration::from_millis(ms));
    }

    fn tracker(&self, slot: usize) -> impl std::ops::Deref<Target = Box<Device>> + use<> {
        lock(&self.frame.slots)
            .get(slot)
            .expect("device registered")
    }

    fn activate_one(&self) {
        self.list(1, &["OpenUT-TestA"]);
        self.run(0);
        self.run(1);
        self.host.take();
    }

    fn reentrant_cleanup(
        &self,
        kind: CallbackKind,
        trigger: impl Fn(&Call) -> bool + Send + Sync + 'static,
    ) {
        let weak = Arc::downgrade(&self.frame);
        self.host.hook(move |call| {
            if trigger(call) {
                let frame = weak.upgrade().expect("frame live");
                frame.cleanup(Some(kind), None);
                assert!(matches!(*lock(&frame.worker), WorkerSlot::Joined));
            }
        });
    }
}

fn poses(calls: Vec<Call>) -> Vec<(u32, Pose)> {
    calls
        .into_iter()
        .filter_map(|call| match call {
            Call::Pose(index, pose) => Some((index, pose)),
            _ => None,
        })
        .collect()
}

#[test]
fn activation_inside_add_or_after_later_frames_uses_the_last_pending_pose() {
    for synchronous in [true, false] {
        let fixture = Fixture::new(synchronous);
        fixture.list(1, &["OpenUT-TestA"]);
        for (seq, x) in [(2, 2.0), (3, 3.0), (4, 4.0)] {
            fixture.pose(seq, 0, x);
        }
        fixture.run(0);
        let calls = fixture.host.take();
        assert_eq!(
            calls.iter().filter(|call| **call == Call::Add(0)).count(),
            1
        );
        assert!(poses(calls).is_empty());
        if !synchronous {
            for ms in [1, 2, 3] {
                fixture.run(ms);
            }
            assert!(fixture.host.take().is_empty());
            assert!(fixture.frame.activate(&fixture.tracker(0), FIRST_INDEX));
        }
        fixture.run(4);
        let updates = poses(fixture.host.take());
        assert_eq!(updates.len(), 1);
        assert_eq!((updates[0].0, updates[0].1.position[0]), (FIRST_INDEX, 4.0));
        fixture.frame.cleanup(None, None);
    }
}

#[test]
fn deactivating_prepared_devices_reentrantly_skips_the_remaining_pose() {
    let fixture = Fixture::new(true);
    fixture.list(1, &["OpenUT-TestA", "OpenUT-TestB"]);
    fixture.run(0);
    fixture.run(1);
    fixture.host.take();

    let weak = Arc::downgrade(&fixture.frame);
    fixture.host.hook(move |call| {
        if matches!(call, Call::Pose(FIRST_INDEX, _)) {
            let frame = weak.upgrade().unwrap();
            for slot in [0, 1] {
                let device = lock(&frame.slots).get(slot).unwrap();
                frame.deactivate(&device);
            }
        }
    });
    fixture.pose(2, 0, 5.0);
    fixture.pose(3, 1, 6.0);
    fixture.run(2);
    assert_eq!(poses(fixture.host.take()).len(), 1);
    assert_eq!(fixture.frame.dropped_actions.load(Ordering::Relaxed), 1);
    fixture.pose(4, 1, 7.0);
    fixture.run(4_000);
    assert!(fixture.host.take().is_empty());
    fixture.frame.cleanup(None, None);
}

#[test]
fn reactivation_does_not_reuse_the_index_of_a_prepared_pose() {
    let fixture = Fixture::new(true);
    fixture.list(1, &["OpenUT-TestA", "OpenUT-TestB"]);
    fixture.run(0);
    fixture.run(1);
    fixture.host.take();

    let frame = Arc::downgrade(&fixture.frame);
    fixture.host.hook(move |call| {
        if matches!(call, Call::Pose(FIRST_INDEX, _)) {
            let frame = frame.upgrade().unwrap();
            let device = lock(&frame.slots).get(1).unwrap();
            frame.deactivate(&device);
            assert!(frame.activate(&device, 99));
        }
    });
    fixture.pose(2, 0, 1.0);
    fixture.pose(3, 1, 2.0);
    fixture.run(2);
    assert_eq!(poses(fixture.host.take()).len(), 1);
    assert_eq!(fixture.frame.dropped_actions.load(Ordering::Relaxed), 1);

    fixture.run(3);
    let updates = poses(fixture.host.take());
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 99);
    fixture.frame.cleanup(None, None);
}

#[test]
fn cross_thread_cleanup_waits_then_drops_two_registrations_and_three_logs() {
    let fixture = Fixture::new(true);
    fixture.activate_one();
    fixture.pose(2, 0, 3.0);
    fixture.list(3, &["OpenUT-TestA", "OpenUT-TestB", "OpenUT-TestC"]);
    for line in ["second", "third", "fourth"] {
        fixture.inbox().push_log(line.to_owned());
    }
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    fixture.host.hook(move |call| {
        if matches!(call, Call::Pose(_, _)) {
            entered_tx.send(()).unwrap();
            lock(&release_rx).recv_timeout(TIMEOUT).unwrap();
        }
    });

    let frame = Arc::clone(&fixture.frame);
    let now = fixture.start + Duration::from_millis(5);
    let run = thread::spawn(move || frame.run_frame(now));
    entered_rx.recv_timeout(TIMEOUT).unwrap();

    let frame = Arc::clone(&fixture.frame);
    let (finished_tx, finished_rx) = mpsc::channel();
    let cleanup = thread::spawn(move || {
        frame.cleanup(None, None);
        finished_tx.send(()).unwrap();
    });
    let wait = Instant::now();
    while fixture.frame.gate.is_open() && wait.elapsed() < TIMEOUT {
        thread::yield_now();
    }
    assert!(!fixture.frame.gate.is_open());
    assert!(finished_rx.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(lock(&fixture.frame.slots).get(0).is_some());

    release_tx.send(()).unwrap();
    finished_rx.recv_timeout(TIMEOUT).unwrap();
    run.join().unwrap();
    cleanup.join().unwrap();
    let calls = fixture.host.take();
    assert_eq!(calls.len(), 1);
    assert!(matches!(calls[0], Call::Pose(FIRST_INDEX, _)));
    assert_eq!(fixture.frame.dropped_actions.load(Ordering::Relaxed), 5);
    assert!(lock(&fixture.frame.slots).iter().next().is_none());
    assert!(lock(&fixture.frame.callbacks).is_none());
    assert!(lock(&fixture.frame.inbox).is_none());
    assert!(matches!(*lock(&fixture.frame.worker), WorkerSlot::Joined));

    fixture.run(10);
    assert!(!fixture.frame.init(fixture.host.clone()));
    assert!(fixture.host.take().is_empty());
}

#[test]
fn cleanup_can_reenter_all_five_host_call_sites_and_still_joins_the_worker() {
    for site in 0..5 {
        let fixture = Fixture::new(site != 3);
        let kind = match site {
            3 => CallbackKind::Activate,
            4 => CallbackKind::Deactivate,
            _ => CallbackKind::RunFrame,
        };

        if site != 1 {
            fixture.activate_one();
        }
        fixture.reentrant_cleanup(kind, move |call| match site {
            0 => matches!(call, Call::Pose(_, _)),
            1 => matches!(call, Call::Add(_)),
            2 => matches!(call, Call::Log(_)),
            3 => matches!(call, Call::Container(_)),
            4 => matches!(call, Call::Log(line) if line.starts_with("openut: Deactivate")),
            _ => false,
        });

        match site {
            0 => {
                fixture.pose(2, 0, 1.0);
                fixture.pose(3, 0, 2.0);
                fixture.run(2);
            }
            1 => {
                fixture.list(1, &["OpenUT-TestA", "OpenUT-TestB"]);
                fixture.run(2);
            }
            2 => {
                fixture.inbox().push_log("first".to_owned());
                fixture.inbox().push_log("second".to_owned());
                fixture.run(2);
            }
            3 => {
                assert!(!fixture.frame.activate(&fixture.tracker(0), FIRST_INDEX));
            }
            4 => {
                fixture.frame.deactivate(&fixture.tracker(0));
            }
            _ => unreachable!(),
        }

        assert_eq!(fixture.host.take().len(), 1);
        assert!(matches!(*lock(&fixture.frame.worker), WorkerSlot::Joined));
        assert!(lock(&fixture.frame.slots).get(0).is_some());
        assert!(lock(&fixture.frame.inbox).is_some());
        fixture.frame.cleanup(None, None);
        assert!(lock(&fixture.frame.slots).get(0).is_some());
        assert!(!fixture.frame.activate(&fixture.tracker(0), FIRST_INDEX));
        assert!(fixture.host.take().is_empty());
    }
}

#[test]
fn cleanup_record_excludes_device_and_directory_names() {
    let source = CleanupSource::Reentrant(CallbackKind::RunFrame);
    let record = cleanup_record(
        UNIX_EPOCH + Duration::from_millis(17),
        source,
        Duration::from_millis(23),
        Duration::from_millis(43),
        false,
    );
    assert_eq!(
        record,
        "utc_unix_ms=17 source=reentrant RunFrame wait_ms=23 join_ms=43 released=false\n",
    );

    let fixture = Fixture::new(false);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "openut-cleanup-check-{}-{nonce}",
        std::process::id()
    ));
    fixture.frame.cleanup(None, Some(&directory));
    let record = fs::read_to_string(directory.join("driver-cleanup.txt")).unwrap();
    assert!(record.contains("source=other-thread frame_active=false"));
    assert!(record.contains("released=true"));
    assert!(!record.contains(directory.to_str().unwrap()));
    fs::remove_dir_all(&directory).unwrap();

    let file = directory.with_extension("file");
    fs::write(&file, "").unwrap();
    fixture.frame.cleanup(None, Some(&file));
    fs::remove_file(file).unwrap();
}
